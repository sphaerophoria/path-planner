// Each degree should be scale steps 
const VERTEX_SHADER_SOURCE =
    `attribute vec2 long_lat; \
     uniform float scale; \
     uniform vec2 center; \
     uniform float aspect_ratio; \ 

     void main(void) { \
         float lat_rad = long_lat.y * 3.1415962 / 180.0; \
         vec2 pos = (long_lat - center) * scale; \
         pos.x = pos.x * cos(lat_rad) / aspect_ratio; \
         gl_Position = vec4(pos, 0.1, 1.0); \
     }`

const FRAGMENT_SHADER_SOURCE =
    `void main(void) { \
      gl_FragColor = vec4(1.0, 1.0, 1.0, 0.1); \
    }`

function _construct_index_buffer(ways) {
    let total_size = 0
    for (let i = 0; i < ways.length; i++) {
        total_size += ways[i].nodes.length
        total_size += 1
    }

    let indices = new Uint32Array(total_size)

    let it = 0
    for (const way of ways) {
        for (let j = 0; j < way.nodes.length; j++) {
            indices[it + j] = way.nodes[j]
        }

        // webgl2 enables primitive restart index by default
        indices[it + way.nodes.length] = 0xffffffff
        it += way.nodes.length + 1
    }

    return indices
}

class Renderer {
    constructor(data) {
        this.canvas = document.getElementById('canvas');
        this.gl = canvas.getContext('webgl2');

        let gl = this.gl
        this.scale = 10.0
        this.center = [-123.1539434, 49.2578263]

        let vertices = new Float32Array(data.nodes.length * 2)
        for (let i = 0; i < data.nodes.length; i++) {
            const node = data.nodes[i]
            vertices[i * 2] = node.long / 10000000.0
            vertices[i * 2 + 1] = node.lat / 10000000.0
        }

        this.vertex_buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);

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

        let indices = _construct_index_buffer(data.ways)
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
    }

    render_map(timestamp) {
        // Update canvas size with window size
        this.canvas.width = document.body.clientWidth
        this.canvas.height = document.body.clientHeight

        let gl = this.gl;

        gl.useProgram(this.shaderProgram);

        let long_lat_loc = gl.getAttribLocation(this.shaderProgram, "long_lat");

        let scale_loc = gl.getUniformLocation(this.shaderProgram, "scale");
        gl.uniform1f(scale_loc, this.scale)

        let center_loc = gl.getUniformLocation(this.shaderProgram, "center");
        gl.uniform2f(center_loc, this.center[0], this.center[1])

        let aspect_ratio_loc = gl.getUniformLocation(this.shaderProgram, "aspect_ratio");
        gl.uniform1f(aspect_ratio_loc, this.canvas.width / this.canvas.height)

        gl.clearColor(0.5, 0.5, 0.5, 0.9);
        gl.viewport(0, 0, this.canvas.width, this.canvas.height);
        gl.clear(gl.COLOR_BUFFER_BIT);

        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);

        gl.vertexAttribPointer(long_lat_loc, 2, gl.FLOAT, false, 0, 0);
        gl.enableVertexAttribArray(long_lat_loc);

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


    _update_overlay(e) {
        let [x, y] = this._pixelToLongLat(e.pageX, e.pageY)
        let elem = document.getElementById("pointer-lat-long")
        elem.textContent = "Lat: " + y + ", Long: " + x
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
        console.log("leave")
        let elem = document.getElementById("overlay")
        elem.style.display = "none"
    }

    _onMouseEnter(e) {
        let elem = document.getElementById("overlay")
        elem.style.display = "block"
        this._update_overlay(e)
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
