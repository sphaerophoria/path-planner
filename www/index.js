// Each degree should be scale steps 
const VERTEX_SHADER_SOURCE =
    `#version 300 es
     precision highp float;
     in vec2 long_lat; \
     in int way_id; \
     in vec3 v_color; \

     flat out int f_way_id; \
     flat out int f_selected_way; \
     out vec3 f_color; \
     uniform float scale; \
     uniform vec2 center; \
     uniform float aspect_ratio; \ 
     uniform int selected_way; \

     void main(void) { \
         float lat_rad = long_lat.y * 3.1415962 / 180.0; \
         vec2 pos = (long_lat - center) * scale; \
         pos.x = pos.x * cos(lat_rad) / aspect_ratio; \
         gl_Position = vec4(pos, 0.1, 1.0); \
         f_way_id = way_id; \
         f_selected_way = selected_way; \
         f_color = v_color; \
     }`

const FRAGMENT_SHADER_SOURCE =
    `#version 300 es
     precision highp float; \
     flat in int f_way_id; \
     flat in int f_selected_way; \
     in vec3 f_color; \
     out vec4 fragColor; \
     void main(void) { \
       if (f_way_id == f_selected_way) {
           fragColor = vec4(1.0, 0.0, 0.0, 1.0); \
       } else { \
           fragColor = vec4(f_color, 1.0); \
       } \
     }`

const WAY_FINDER_FRAG_SOURCE =
    `#version 300 es
     precision highp float;
     flat in int f_way_id; \
     out int fragColor; \
     void main(void) { \
       // This is some abuse. readPixels needs an RGBA/uint8 output, so we just
       // coerce our 32 bit int into it
       fragColor = f_way_id; \
     }`

function _getRGB(input) {
    if (input.substr(0,1)=="#") {
    var collen=(input.length-1)/3;
    var fact=[17,1,0.062272][collen-1];
    return [
        Math.round(parseInt(input.substr(1,collen),16)*fact) / 255.0,
        Math.round(parseInt(input.substr(1+collen,collen),16)*fact) / 255.0,
        Math.round(parseInt(input.substr(1+2*collen,collen),16)*fact) / 255.0
    ];
    }
    else return input.split("(")[1].split(")")[0].split(",").map(x=>+x);
}

function _way_to_color(way) {
    const custom_regex = document.getElementById("custom-highlight-regex").value
    let custom_color = _getRGB(document.getElementById("custom-highlight-color").value)

    const re = new RegExp(custom_regex)
    let deferred_color = null
    for (tag of way.tags) {
        if (custom_regex.length > 3 && tag.match(re)) {
            return custom_color
        }
        else if (tag.startsWith("cycleway")) {
            let value = tag.substring(tag.indexOf('/') + 1)

            if (value == "no") {
                continue
            }

            deferred_color = [0.0, 1.0, 0.0]
        } else if (
                tag == "highway/cycleway" ||
                tag == "bicycle/designated" ||
                tag == "bicycle/yes") {
            deferred_color = [0.0, 1.0, 0.0]
        }
    }

    if (deferred_color !== null) {
        return deferred_color;
    } else {
        return [1.0, 1.0, 1.0]

    }
}

function _constructMapBuffers(data) {
    // vertex buffer has a unique vertex for shared node ids in a way. This
    // allows us to attach info about a way to the render. E.g. color for
    // different tag types. We still need to use an index buffer though, that
    // essentially just is just a linear increase split by 0xffff. This allows
    // us to use glDrawElements with primitive restart and draw the whole map
    // in a single draw call

    let index_size = 0
    let vertex_size = 0
    for (let i = 0; i < data.ways.length; i++) {
        index_size += data.ways[i].nodes.length
        index_size += 1
        vertex_size += data.ways[i].nodes.length * 6 * 4
    }

    let indices = new Uint32Array(index_size)
    let vertices = new ArrayBuffer(vertex_size)
    let float_vertices = new Float32Array(vertices)
    let int_vertices = new Int32Array(vertices)

    let vertex_it = 0
    let index_it = 0
    for (let i = 0; i < data.ways.length; i++) {
        let way = data.ways[i]

        let color = _way_to_color(way)
        for (let j = 0; j < way.nodes.length; j++) {
            let node_id = way.nodes[j]
            // Data is in decimicro degrees, but we just convert to lower
            // precision floats because it's easier  to think about, has good
            // interop with webgl, and doesn't matter
            float_vertices[vertex_it * 6] = data.nodes[node_id].long / 10000000.0
            float_vertices[vertex_it * 6 + 1] = data.nodes[node_id].lat / 10000000.0
            int_vertices[vertex_it * 6 + 2] = i
            // Color
            float_vertices[vertex_it * 6 + 3] = color[0]
            float_vertices[vertex_it * 6 + 4] = color[1]
            float_vertices[vertex_it * 6 + 5] = color[2]
            indices[index_it] = vertex_it
            vertex_it += 1
            index_it += 1
        }

        // webgl2 enables primitive restart index by default
        indices[index_it] = 0xffffffff
        index_it += 1
    }

    return [vertices, indices]
}


class Renderer {
    constructor(data) {
        this.canvas = document.getElementById('canvas');
        this.gl = canvas.getContext('webgl2');

        let gl = this.gl
        this.scale = 10.0
        this.center = [-123.1539434, 49.2578263]
        this.data = data
        this.selected_way_id = -1

        let vertShader = gl.createShader(gl.VERTEX_SHADER);
        gl.shaderSource(vertShader, VERTEX_SHADER_SOURCE);
        gl.compileShader(vertShader);

        let fragShader = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(fragShader, FRAGMENT_SHADER_SOURCE);
        gl.compileShader(fragShader);

        this.shaderProgram = gl.createProgram();
        gl.attachShader(this.shaderProgram, vertShader);
        gl.attachShader(this.shaderProgram, fragShader);
        gl.linkProgram(this.shaderProgram);

        let wayFinderShader = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(wayFinderShader, WAY_FINDER_FRAG_SOURCE);
        gl.compileShader(wayFinderShader);

        this.wayFinderProgram = gl.createProgram()
        gl.attachShader(this.wayFinderProgram, vertShader);
        gl.attachShader(this.wayFinderProgram, wayFinderShader);
        gl.linkProgram(this.wayFinderProgram);

        this.wayFinderTexture = gl.createRenderbuffer();
        gl.bindRenderbuffer(gl.RENDERBUFFER, this.wayFinderTexture)
        gl.renderbufferStorage(gl.RENDERBUFFER, gl.R32I, 2, 2)

        this.wayFinderBuffer = gl.createFramebuffer();
        gl.bindFramebuffer(gl.FRAMEBUFFER, this.wayFinderBuffer)
        gl.framebufferRenderbuffer(gl.DRAW_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.RENDERBUFFER, this.wayFinderTexture)
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);

        let [vertices, indices] = _constructMapBuffers(data)

        this.vertex_buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);

        this.index_buffer = gl.createBuffer();
        this.index_buffer_length = indices.length;
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, null)


        let canvasHolder = document.getElementById('canvas-holder')
        canvasHolder.onmousemove = this.onMouseMove.bind(this)
        canvasHolder.addEventListener('wheel', this.onScroll.bind(this))

        canvasHolder.onmouseenter = this._onMouseEnter.bind(this)
        canvasHolder.onmouseleave = this._onMouseLeave.bind(this)

        document.getElementById('custom-highlight-regex').addEventListener('input', this._onCustomHighlightChanged.bind(this))
        document.getElementById('custom-highlight-color').addEventListener('input', this._onCustomHighlightChanged.bind(this))
    }

    render_map(timestamp) {
        // Update canvas size with window size
        this.canvas.width = document.body.clientWidth
        this.canvas.height = document.body.clientHeight

        let gl = this.gl;

        gl.useProgram(this.shaderProgram);

        let long_lat_loc = gl.getAttribLocation(this.shaderProgram, "long_lat");
        let way_id_loc = gl.getAttribLocation(this.shaderProgram, "way_id");
        let color_loc = gl.getAttribLocation(this.shaderProgram, "v_color");

        let scale_loc = gl.getUniformLocation(this.shaderProgram, "scale");
        gl.uniform1f(scale_loc, this.scale)

        let center_loc = gl.getUniformLocation(this.shaderProgram, "center");
        gl.uniform2f(center_loc, this.center[0], this.center[1])

        let aspect_ratio_loc = gl.getUniformLocation(this.shaderProgram, "aspect_ratio");
        gl.uniform1f(aspect_ratio_loc, this.canvas.width / this.canvas.height)

        let selected_way_loc = gl.getUniformLocation(this.shaderProgram, "selected_way");
        gl.uniform1i(selected_way_loc, this.selected_way_id)

        gl.clearColor(0.5, 0.5, 0.5, 0.9);
        gl.viewport(0, 0, this.canvas.width, this.canvas.height);
        gl.clear(gl.COLOR_BUFFER_BIT);

        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);

        let num_elements = 6
        gl.vertexAttribPointer(long_lat_loc, 2, gl.FLOAT, false, 4 * num_elements, 0);
        gl.enableVertexAttribArray(long_lat_loc);

        gl.vertexAttribIPointer(way_id_loc, 1, gl.INT, 4 * num_elements, 8);
        gl.enableVertexAttribArray(way_id_loc);

        gl.vertexAttribPointer(color_loc, 3, gl.FLOAT, false, 4 * num_elements, 12);
        gl.enableVertexAttribArray(color_loc);

        gl.drawElements(gl.LINE_STRIP, this.index_buffer_length, gl.UNSIGNED_INT, 0);
    }

    onScroll(e) {
        // Scale needs to increase faster as we get closer, and always needs to
        // be above 0. Pick a f(s, x) where 
        // f(s, 0) = s
        // f(s, -y) < s
        // f(s, y) > s
        // f(s, y) > 0
        // and f(1000, y) >>= f(10, y)
        // Hand wavey, multiplying by an exponential function seems to feel
        // fine. No mathematical reason to use this over anything else that
        // satisfies the above constraints
        this.scale *= Math.pow(1.001, -e.deltaY)
        window.requestAnimationFrame(this.render_map.bind(this))
        return true
    }

    onMouseMove(e) {
        this._update_overlay(e)
        if (e.buttons == 0) {
            window.requestAnimationFrame(this.render_map.bind(this))
            return
        }

        let [start_long, start_lat] = this._pixelToLongLat(
            e.pageX + e.movementX,
            e.pageY + e.movementY)

        let [end_long, end_lat] = this._pixelToLongLat(e.pageX, e.pageY)
        let x_movement_long = end_long - start_long
        let y_movement_lat = end_lat - start_lat
        this.center[0] += x_movement_long
        this.center[1] += y_movement_lat
        window.requestAnimationFrame(this.render_map.bind(this))
    }

    _findTagForLongLat(long, lat) {

        // This is what I would consider to be a colossal hack
        //
        // In order to figure out which "way" is closest to us, we don't really
        // have a choice but to check our proximity to every way. We could
        // pre-process this data to try to create "cells" of ways that reduce
        // the amount of checks, but there's a lot of complexity there.
        //
        // If we check every way in javascript, doing a simple axis aligned
        // bbox check, it takes 174ms. That's completely unreasonable if we
        // want to do it every frame or on every mouse move.
        //
        // The good news is we already render every single road on every frame,
        // and that's incredibly fast. The GPU is handling everything for us.
        // If we can somehow leverage that power in WebGL we get the answer
        // we're looking for.
        //
        // So we...
        // * Use our usual map drawing vertex shader, but we center the
        //   "viewport" on a very zoomed in segment near the provided longitude
        //   and latitude
        // * We use a special fragment shader that just outputs the way id
        //   directly
        // * We render that scene to a 1x1 pixel render buffer. If there are
        //   multiple ways in the viewport this should just pick one at random
        // * We read back that single pixel, and do some bit shifting hackery
        //   to extract a 32bit way id from a RGBA8 color
        //
        // This takes around 1.2ms on machine vs the 174 I tested with the
        // simple javascript approach

        let gl = this.gl;
        gl.useProgram(this.wayFinderProgram);

        let long_lat_loc = gl.getAttribLocation(this.wayFinderProgram, "long_lat");
        let way_id_loc = gl.getAttribLocation(this.wayFinderProgram, "way_id");

        let scale_loc = gl.getUniformLocation(this.wayFinderProgram, "scale");
        gl.uniform1f(scale_loc, this.scale * 50)

        let center_loc = gl.getUniformLocation(this.wayFinderProgram, "center");
        gl.uniform2f(center_loc, long, lat)

        let aspect_ratio_loc = gl.getUniformLocation(this.wayFinderProgram, "aspect_ratio");
        gl.uniform1f(aspect_ratio_loc, this.canvas.width / this.canvas.height)

        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, this.wayFinderBuffer)

        gl.viewport(0, 0, 1, 1);
        gl.clearBufferiv(gl.COLOR, 0, new Uint32Array([-1, -1, -1, -1]))

        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);

        let num_elements = 6;
        gl.vertexAttribPointer(long_lat_loc, 2, gl.FLOAT, false, 4 * num_elements, 0);
        gl.enableVertexAttribArray(long_lat_loc);

        gl.vertexAttribIPointer(way_id_loc, 1, gl.INT, 4 * num_elements, 8);
        gl.enableVertexAttribArray(way_id_loc);

        gl.drawElements(gl.LINE_STRIP, this.index_buffer_length, gl.UNSIGNED_INT, 0);

        // Duplicate vertices and assign a way ID
        // Set the center of the image to be where the mouse is
        // Render a 1x1 image with scale / 100
        // Check "color" of the output
        let pixels = new Int32Array(4)
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.wayFinderBuffer)
        gl.readPixels(0, 0, 1, 1, gl.RGBA_INTEGER, gl.INT, pixels)
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);

        let way_id = pixels[0]
        return way_id
    }

    _update_overlay(e) {
        let [x, y] = this._pixelToLongLat(e.pageX, e.pageY)
        let way_id = this._findTagForLongLat(x, y)
        let elem = document.getElementById("pointer-lat-long")

        elem.innerHTML = ""

        if (way_id != -1) {
            for (let tag of this.data.ways[way_id].tags) {
                elem.innerHTML += tag
                elem.innerHTML += "<br>"
            }
        }
        elem.innerHTML += "Lat: " + y + "<br>Long: " + x
        this.selected_way_id = way_id
    }

    _pixelToLongLat(x, y) {
        let x_rel = ((x / this.canvas.width) * 2.0 - 1.0) * this.canvas.width / this.canvas.height
        let y_rel = ((1.0 - y / this.canvas.height) * 2.0 - 1.0)

        // We also scale our image with our latitude so that our map has 1
        // degree x ~= 1 degree y in distance. We can be pretty approximate
        // here.
        let x_long_rel = (
            x_rel / this.scale / Math.cos(this.center[1] * 3.14159 / 180.0)
        )

        let y_lat_rel = y_rel / this.scale

        return [x_long_rel + this.center[0], y_lat_rel + this.center[1]]
    }

    _onMouseLeave(e) {
        let elem = document.getElementById("overlay")
        elem.style.display = "none"
        this.selected_way_id = -1
        window.requestAnimationFrame(this.render_map.bind(this))

    }

    _onMouseEnter(e) {
        let elem = document.getElementById("overlay")
        elem.style.display = "block"
    }

    _onCustomHighlightChanged() {
        console.log("Recoloring")
        let [vertices, indices] = _constructMapBuffers(this.data)
        let gl = this.gl

        this.vertex_buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);

        this.index_buffer = gl.createBuffer();
        this.index_buffer_length = indices.length;
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, null)

        window.requestAnimationFrame(this.render_map.bind(this))
    }
}

async function init() {
    let resp = await fetch("/data.json")
    let data = await resp.json()
    let renderer = new Renderer(data)
    window.requestAnimationFrame(renderer.render_map.bind(renderer))
    window.onresize = renderer.render_map.bind(renderer)
}


init()
