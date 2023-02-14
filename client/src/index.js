"use strict"

// Each degree should be scale steps 
const VERTEX_SHADER_SOURCE =
    `#version 300 es
     precision highp float; \
     precision highp int; \
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
         gl_PointSize = 5.0; \
         f_way_id = way_id; \
         f_selected_way = selected_way; \
         f_color = v_color; \
     }`

const FRAGMENT_SHADER_SOURCE =
    `#version 300 es
     precision highp float; \
     precision highp int; \
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
     precision highp float; \
     precision highp int; \
     flat in int f_way_id; \
     out int fragColor; \
     void main(void) { \
       // This is some abuse. readPixels needs an RGBA/uint8 output, so we just
       // coerce our 32 bit int into it
       fragColor = f_way_id; \
     }`

const WAY_FINDER_RES = 11

function _contextMenuVisible() {
    let context_menu = document.getElementById("context-menu")
    return window.getComputedStyle(context_menu).display == "block"
}

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

function _wayToColor(way) {
    const custom_regex = document.getElementById("custom-highlight-regex").value
    let custom_color = _getRGB(document.getElementById("custom-highlight-color").value)

    const re = new RegExp(custom_regex)
    let deferred_color = null
    for (let tag of way.tags) {
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

        let color = _wayToColor(way)
        for (let j = 0; j < way.nodes.length; j++) {
            let node_id = way.nodes[j]
            // Data is in decimicro degrees, but we just convert to lower
            // precision floats because it's easier  to think about, has good
            // interop with webgl, and doesn't matter
            let longlat = _nodeToLongLat(data.nodes[node_id])
            float_vertices[vertex_it * 6] = longlat[0]
            float_vertices[vertex_it * 6 + 1] = longlat[1]
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

function _constructSinglePointBuffer(long, lat, color) {
    let vertices = new ArrayBuffer(24)
    let float_vertices = new Float32Array(vertices)
    let int_vertices = new Int32Array(vertices)

    float_vertices[0] = long
    float_vertices[1] = lat
    int_vertices[2] = -1
    // Color
    float_vertices[3] = color[0]
    float_vertices[4] = color[1]
    float_vertices[5] = color[2]

    return vertices
}

function _constructPathRenderBuffer(path) {
    let vertices = new ArrayBuffer(24 * path.length)
    let float_vertices = new Float32Array(vertices)
    let int_vertices = new Int32Array(vertices)

    for (let i = 0; i < path.length; i++) {
        let long = path[i][0]
        let lat = path[i][1]

        float_vertices[6 * i] = long
        float_vertices[6 * i + 1] = lat
        int_vertices[6 * i + 2] = -1
        // Color
        float_vertices[6 * i + 3] = 0.0
        float_vertices[6 * i + 4] = 0.0
        float_vertices[6 * i + 5] = 1.0

    }

    return vertices
}

function _pixelFromBuffer(pixels, x, y) {
    let idx = 4 * (y * WAY_FINDER_RES + x)
    let ret = pixels[idx]
    return ret
}

function _spiralSearchWayId(pixels) {
    let center_val = Math.floor(WAY_FINDER_RES / 2)
    for (let dist = 0; dist < center_val + 1; dist++) {
        let lower_idx = center_val - dist
        let higher_idx = center_val + dist

        for (let x = lower_idx; x <= higher_idx; x++) {
            let way_id = _pixelFromBuffer(pixels, x, lower_idx)
            if (way_id != -1) {
                return way_id
            }

            way_id = _pixelFromBuffer(pixels, x, higher_idx)
            if (way_id != -1) {
                return way_id
            }
        }

        for (let y = lower_idx; y <= higher_idx; y++) {
            let way_id = _pixelFromBuffer(pixels, lower_idx, y)
            if (way_id != -1) {
                return way_id
            }
            way_id = _pixelFromBuffer(pixels, higher_idx, y)
            if (way_id != -1) {
                return way_id
            }
        }
    }

    return -1
}


function _findWayPosition(data, way_id, long, lat) {
    if (way_id == -1) {
        return [null, null]
    }

    // Distance fn for line
    // dist([0, 1]) = 
    // x = x1 + (x2 - x1) * i
    // y = y1 + (y2 - y1) * i
    // dist_2 = x * x + y * y
    // Walk the line in 10% chunks, find the shortest distance

    let min_dist_2 = Infinity
    let min_dist_node = null
    let min_dist_factor = null

    let way_nodes = data.ways[way_id].nodes
    for (let way_segment_it = 0;
             way_segment_it < way_nodes.length - 1;
             way_segment_it++) {
        let n1 = data.nodes[way_nodes[way_segment_it]]
        let n2 = data.nodes[way_nodes[way_segment_it + 1]]

        for (let i = 0.0; i <= 1.0; i += 0.1) {
            let n1_longlat = _nodeToLongLat(n1)
            let n2_longlat = _nodeToLongLat(n2)

            let way_long = (n2_longlat[0] - n1_longlat[0]) * i + n1_longlat[0]
            let way_lat = (n2_longlat[1] - n1_longlat[1]) * i + n1_longlat[1]

            let long_dist = way_long - long
            let lat_dist = way_lat - lat

            let dist_2 = long_dist * long_dist + lat_dist * lat_dist
            if (dist_2 < min_dist_2) {
                min_dist_2 = dist_2
                min_dist_node = way_segment_it
                min_dist_factor = i
            }
        }
    }

    return [min_dist_node, min_dist_factor]
}

function _wayPositionToLongLat(data, way_position) {
    let way = data.ways[way_position[0]]
    let n1_id = way.nodes[way_position[1]]
    let n2_id = way.nodes[way_position[1] + 1]
    let factor = way_position[2]
    let n1 = data.nodes[n1_id]
    let n2 = data.nodes[n2_id]

    let long_decimicro = (n2.long - n1.long) * factor + n1.long
    let lat_decimicro = (n2.lat - n1.lat) * factor + n1.lat

    return [long_decimicro / 10000000.0, lat_decimicro / 10000000.0]
}

function _setVertexAttribPointers(gl, program) {
    let long_lat_loc = gl.getAttribLocation(program, "long_lat");
    let way_id_loc = gl.getAttribLocation(program, "way_id");
    let color_loc = gl.getAttribLocation(program, "v_color");

    let num_elements = 6
    gl.vertexAttribPointer(long_lat_loc, 2, gl.FLOAT, false, 4 * num_elements, 0);
    gl.enableVertexAttribArray(long_lat_loc);

    gl.vertexAttribIPointer(way_id_loc, 1, gl.INT, 4 * num_elements, 8);
    gl.enableVertexAttribArray(way_id_loc);

    gl.vertexAttribPointer(color_loc, 3, gl.FLOAT, false, 4 * num_elements, 12);
    gl.enableVertexAttribArray(color_loc);
}


function _neighbors(data, node_id) {
    let neighbors = []
    for (let node_to_way of data.node_to_way[node_id]) {
        let [way_id, sub_id] = node_to_way
        let way = data.ways[way_id]
        if (sub_id + 1 != way.nodes.length) {
            neighbors.push(way.nodes[sub_id + 1])
        }

        if (sub_id != 0) {
            neighbors.push(way.nodes[sub_id - 1])
        }
    }

    return neighbors
}

function _nodeToLongLat(node) {
    return [node.long / 10000000.0, node.lat / 10000000.0]
}

function _distance(n1, n2) {
    let long_dist = (n2.long - n1.long)
    let lat_dist = (n2.lat - n1.lat)

    long_dist = long_dist * Math.cos(n2.lat / 10000000.0 * 3.1415962 / 180.0)
    long_dist /= 10000000.0
    lat_dist /= 10000000.0

    return Math.sqrt(long_dist * long_dist + lat_dist * lat_dist)
}

function _insertIntoOpenSet(open_set, f_scores, val) {
    if (open_set.length == 0) {
        open_set.push(val)
    }

    let last_less = 0
    let last_greater = open_set.length

    let val_score = f_scores.get(val)

    while (last_greater - last_less > 1) {
        let test_idx = Math.floor((last_less + last_greater) / 2)
        let test_score = f_scores.get(open_set[test_idx])
        if (test_score < val_score) {
            last_less = test_idx
        }
        // Bias towards inserting at front
        if (test_score >= val_score) {
            last_greater = test_idx
        }
    }


    open_set.splice(last_less, 0, val)
}

function _minItem(open_set, f_score) {
    let min_item = null
    for (let item of open_set) {
        if (min_item === null) {
            min_item = item
            continue
        }

        if (f_score.get(item) < f_score.get(min_item)) {
            min_item = item
        }
    }

    return min_item
}

function _nodeIdToLongLat(data, node_id) {
    let node = data.nodes[node_id]

    return [node.long / 10000000.0, node.lat / 10000000.0]

}

function _reconstructPath(data, came_from, current) {
    let total_path = [_nodeIdToLongLat(data, current)]
    while (came_from.has(current)) {
        current = came_from.get(current)
        total_path.push(_nodeIdToLongLat(data, current))
    }

    return total_path
}

function _planRoute(data, start, end, debug_paths) {
    if (start === null || end === null) {
        return null
    }

    let start_node = data.ways[start[0]].nodes[start[1]]
    let end_node = data.ways[end[0]].nodes[end[1]]

    let open_set = [start_node]
    let came_from = new Map()
    let g_score = new Map();
    g_score.set(start_node, 0)

    let f_score = new Map();
    f_score.set(start_node, _distance(data.nodes[start_node], data.nodes[end_node]))

    let i = 0
    // Limit to 50k nodes, or else we start to get unbearably slow
    let max_attempts = document.getElementById("max-path-nodes").value
    while (open_set.length != 0 && i < max_attempts) {
        i += 1

        let item = open_set.shift()

        if (item == end_node) {
            if (debug_paths == false) {
                return _reconstructPath(data, came_from, item)
            } else {
                break
            }
        }

        for (let neighbor of _neighbors(data, item)) {
            let neighbor_distance = _distance(data.nodes[item], data.nodes[neighbor])

            let tentative_g_score = g_score.get(item) + neighbor_distance

            if (g_score.has(neighbor) == false) {
                g_score.set(neighbor, Infinity)
            }

            if (tentative_g_score < g_score.get(neighbor)) {
                came_from.set(neighbor, item)
                g_score.set(neighbor, tentative_g_score)
                f_score.set(neighbor, tentative_g_score + _distance(data.nodes[neighbor], data.nodes[end_node]))

                _insertIntoOpenSet(open_set, f_score, neighbor)
            }

        }
    }

    if (debug_paths == true) {
        let ret = []
        for (let [key, value] of f_score) {
            ret.push(_nodeToLongLat(data.nodes[key]))
        }
        return ret
    } else {
        return null
    }
}

function _preprocessData(data) {
    let node_to_way = Array(data.nodes.length)
    for (let way_id = 0; way_id < data.ways.length; way_id++) {
        let way = data.ways[way_id]
        for (let i = 0; i < way.nodes.length; i++) {
            let node_id = way.nodes[i]
            if (node_to_way[node_id] === undefined) {
                node_to_way[node_id] = []
            }

            node_to_way[node_id].push([way_id, i])
        }
    }

    data.node_to_way = node_to_way;
}

class PointerTracker {
    constructor(renderer) {
        let canvas_holder = document.getElementById('canvas-holder')
        canvas_holder.addEventListener('pointerdown', this._onPointerDown.bind(this))
        canvas_holder.addEventListener('pointermove', this._onPointerMove.bind(this))
        canvas_holder.addEventListener('pointerup', this._onPointerUp.bind(this))

        this.renderer = renderer

        this.down_pointers = []
        this.scale = 1.0
    }

    _calculatePointerDistance(e1, e2) {
        let y_dist = e2.pageY - e1.pageY
        let y_dist_2 = y_dist * y_dist

        let x_dist = e2.pageX - e1.pageX
        let x_dist_2 = x_dist * x_dist
        return Math.sqrt(x_dist_2 + y_dist_2)
    }

    _onPointerUp(e) {
        let idx = this._pointerIdx(e)
        this.down_pointers.splice(idx, 1)
    }

    _onPointerMove(e) {
        if (_contextMenuVisible()) {
            return
        }

        let idx = this._pointerIdx(e)

        this.renderer.updatePointerPos(e.pageX, e.pageY)
        if (this.down_pointers.length == 1) {
            let last_event = this.down_pointers[idx]
            this.down_pointers[idx] = e

            this.renderer.moveViewport(e.pageX, e.pageY, e.pageX - last_event.pageX, e.pageY - last_event.pageY)
        }
        else if (this.down_pointers.length == 2) {
            let other_idx = (idx + 1) % 2
            let last_event = this.down_pointers[idx]
            let other_event = this.down_pointers[other_idx]
            this.down_pointers[idx] = e
            let old_distance = this._calculatePointerDistance(other_event, last_event)
            let new_distance = this._calculatePointerDistance(other_event, e)
            // Scale increase is a little off right now, we should set the
            // delta in pixel space, and then set the scale and center so that
            // the distance between fingers stays the same distance on the map...
            // For now though this is good enough
            let delta = (new_distance - old_distance) * 2
            this.renderer.updateScale(e.pageX, e.pageY, delta)
        }
    }

    _onPointerDown(e) {
        let idx = this._pointerIdx(e)
        this.down_pointers[idx] = e
        this.renderer.updatePointerPos(e.pageX, e.pageY)
    }

    _pointerIdx(e) {
        let idx = this.down_pointers.findIndex((cached_ev) => cached_ev.pointerId == e.pointerId)
        if (idx == -1) {
            return this.down_pointers.length
        }

        return idx
    }
}


class Renderer {
    constructor(data) {
        this.canvas = document.getElementById('canvas');
        this.gl = canvas.getContext('webgl2');

        let gl = this.gl
        this.scale = 10.0
        this.center = [-123.1539434, 49.2578263]
        this.data = data
        this.selected_way = null
        this.start_position = null
        this.planned_path = null
        this.debug_paths = false

        let vert_shader = gl.createShader(gl.VERTEX_SHADER);
        gl.shaderSource(vert_shader, VERTEX_SHADER_SOURCE);
        gl.compileShader(vert_shader);

        let frag_shader = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(frag_shader, FRAGMENT_SHADER_SOURCE);
        gl.compileShader(frag_shader);

        this.shader_program = gl.createProgram();
        gl.attachShader(this.shader_program, vert_shader);
        gl.attachShader(this.shader_program, frag_shader);
        gl.linkProgram(this.shader_program);

        let way_finder_shader = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(way_finder_shader, WAY_FINDER_FRAG_SOURCE);
        gl.compileShader(way_finder_shader);

        this.way_finder_program = gl.createProgram()
        gl.attachShader(this.way_finder_program, vert_shader);
        gl.attachShader(this.way_finder_program, way_finder_shader);
        gl.linkProgram(this.way_finder_program);

        this.way_finder_render_buffer = gl.createRenderbuffer();
        gl.bindRenderbuffer(gl.RENDERBUFFER, this.way_finder_render_buffer)
        gl.renderbufferStorage(gl.RENDERBUFFER, gl.R32I, WAY_FINDER_RES, WAY_FINDER_RES)

        this.way_finder_frame_buffer = gl.createFramebuffer();
        gl.bindFramebuffer(gl.FRAMEBUFFER, this.way_finder_frame_buffer)
        gl.framebufferRenderbuffer(gl.DRAW_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.RENDERBUFFER, this.way_finder_render_buffer)
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);

        let [vertices, indices] = _constructMapBuffers(data)

        this.vertex_array = gl.createVertexArray()
        gl.bindVertexArray(this.vertex_array)

        this.vertex_buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);

        this.index_buffer = gl.createBuffer();
        this.index_buffer_length = indices.length;
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

        _setVertexAttribPointers(gl, this.shader_program)

        gl.bindVertexArray(null)
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, null)
        gl.bindBuffer(gl.ARRAY_BUFFER, null);

        this.single_point_vertex_array = gl.createVertexArray()
        gl.bindVertexArray(this.single_point_vertex_array)
        this.waypoint_buffer = gl.createBuffer()
        gl.bindBuffer(gl.ARRAY_BUFFER, this.waypoint_buffer)

        _setVertexAttribPointers(gl, this.shader_program)

        gl.bindVertexArray(null)

        let canvas_holder = document.getElementById('canvas-holder')
        canvas_holder.addEventListener('wheel', this._onScroll.bind(this))

        canvas_holder.onmouseenter = this._onMouseEnter.bind(this)
        canvas_holder.onmouseleave = this._onMouseLeave.bind(this)
        canvas_holder.onclick = this._onLeftClick.bind(this)
        canvas_holder.oncontextmenu = this._onRightClick.bind(this)

        document.getElementById('custom-highlight-regex').addEventListener('input', this._onCustomHighlightChanged.bind(this))
        document.getElementById('custom-highlight-color').addEventListener('input', this._onCustomHighlightChanged.bind(this))

        document.getElementById('start-route').onclick = this._setStartPos.bind(this)
        document.getElementById('clear-route').onclick = this._clearStartPos.bind(this)

        document.getElementById('debug-path').onclick = () => { this.debug_paths = !this.debug_paths }
        window.requestAnimationFrame(this.render_map.bind(this))
        window.onresize = this.render_map.bind(this)
    }

    render_map() {
        // Update canvas size with window size
        this.canvas.width = document.body.clientWidth
        this.canvas.height = document.body.clientHeight

        let gl = this.gl;

        gl.useProgram(this.shader_program);

        let scale_loc = gl.getUniformLocation(this.shader_program, "scale");
        gl.uniform1f(scale_loc, this.scale)

        let center_loc = gl.getUniformLocation(this.shader_program, "center");
        gl.uniform2f(center_loc, this.center[0], this.center[1])

        let aspect_ratio_loc = gl.getUniformLocation(this.shader_program, "aspect_ratio");
        gl.uniform1f(aspect_ratio_loc, this.canvas.width / this.canvas.height)

        let selected_way_loc = gl.getUniformLocation(this.shader_program, "selected_way");
        if (this.selected_way !== null) {
            gl.uniform1i(selected_way_loc, this.selected_way[0])
        } else {
            gl.uniform1i(selected_way_loc, -1)
        }

        gl.clearColor(0.5, 0.5, 0.5, 0.9);
        gl.viewport(0, 0, this.canvas.width, this.canvas.height);
        gl.clear(gl.COLOR_BUFFER_BIT);

        gl.bindVertexArray(this.vertex_array)
        gl.drawElements(gl.LINE_STRIP, this.index_buffer_length, gl.UNSIGNED_INT, 0);

        if (this.selected_way !== null) {
            gl.bindVertexArray(this.single_point_vertex_array)
            let [long, lat] = _wayPositionToLongLat(this.data, this.selected_way)
            let verts = _constructSinglePointBuffer(long, lat, [1.0, 0.0, 0.0])
            gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
            gl.drawArrays(gl.POINTS, 0, 1);
            gl.bindVertexArray(null)
        }

        if (this.start_position !== null) {
            gl.bindVertexArray(this.single_point_vertex_array)
            let [long, lat] = _wayPositionToLongLat(this.data, this.start_position)
            let verts = _constructSinglePointBuffer(long, lat, [0.0, 0.0, 1.0])
            gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
            gl.drawArrays(gl.POINTS, 0, 1);
            gl.bindVertexArray(null)

        }

        if (this.planned_path !== null) {
            gl.bindVertexArray(this.single_point_vertex_array)
            let verts = _constructPathRenderBuffer(this.planned_path)
            gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
            let draw_type = (this.debug_paths == true) ? gl.POINTS : gl.LINE_STRIP
            gl.drawArrays(draw_type, 0, this.planned_path.length);
            gl.bindVertexArray(null)

        }
    }

    updateScale(x, y, delta) {
        let [mouse_long, mouse_lat] = this._pixelToLongLat(x, y)

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
        this.scale *= Math.pow(1.001, delta)

        // Adjust center so that at the new scale, the pointer stays at the
        // same latitude and longitude
        let [new_mouse_long, new_mouse_lat] = this._pixelToLongLat(x, y)
        this.center[0] -= new_mouse_long - mouse_long
        this.center[1] -= new_mouse_lat - mouse_lat
        window.requestAnimationFrame(this.render_map.bind(this))
    }

    _onScroll(e) {
        this.updateScale(e.pageX, e.pageY, -e.deltaY)
        return true
    }

    moveViewport(x, y, old_x, old_y) {
        let [start_long, start_lat] = this._pixelToLongLat(
            x - old_x,
            y - old_y)

        let [end_long, end_lat] = this._pixelToLongLat(x, y)
        let x_movement_long = end_long - start_long
        let y_movement_lat = end_lat - start_lat
        this.center[0] -= x_movement_long
        this.center[1] -= y_movement_lat

        window.requestAnimationFrame(this.render_map.bind(this))
    }

    _findWayNearLongLat(long, lat) {

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
        // * We render that scene to a 11x11 pixel render buffer
        // * We iterate over the 121 pixels to find the way ID closest to the
        //   center
        // * If we don't find anything we zoom out and try again

        let gl = this.gl;
        gl.useProgram(this.way_finder_program);

        let scale_loc = gl.getUniformLocation(this.way_finder_program, "scale");
        let center_loc = gl.getUniformLocation(this.way_finder_program, "center");
        let aspect_ratio_loc = gl.getUniformLocation(this.way_finder_program, "aspect_ratio");
        gl.uniform2f(center_loc, long, lat)
        gl.uniform1f(aspect_ratio_loc, this.canvas.width / this.canvas.height)

        gl.viewport(0, 0, WAY_FINDER_RES, WAY_FINDER_RES);
        gl.bindVertexArray(this.vertex_array)

        let scale = this.scale * 50
        // Arbitrary cutoff
        // NOTE: The farther away from the cursor we get, the less accurate this becomes
        while (scale > 50.0) {
            gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, this.way_finder_frame_buffer)
            gl.uniform1f(scale_loc, scale)
            gl.clearBufferiv(gl.COLOR, 0, new Uint32Array([-1, -1, -1, -1]))
            gl.drawElements(gl.LINE_STRIP, this.index_buffer_length, gl.UNSIGNED_INT, 0);
            let pixels = new Int32Array(4 * WAY_FINDER_RES * WAY_FINDER_RES)
            gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.way_finder_frame_buffer)
            gl.readPixels(0, 0, WAY_FINDER_RES, WAY_FINDER_RES, gl.RGBA_INTEGER, gl.INT, pixels)
            gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);

            let way_id = _spiralSearchWayId(pixels)
            let [way_segment, way_factor] = _findWayPosition(this.data, way_id, long, lat)
            if (way_id != -1) {
                return [way_id, way_segment, way_factor]
            }

            scale /= 2
        }
        return null
    }

    updatePointerPos(x, y) {
        let [long, lat] = this._pixelToLongLat(x, y)
        this.selected_way = this._findWayNearLongLat(long, lat)
        let elem = document.getElementById("pointer-lat-long")

        this._setSearchEnd(this.selected_way)

        elem.innerHTML = ""

        if (this.selected_way != null) {
            for (let tag of this.data.ways[this.selected_way[0]].tags) {
                elem.innerHTML += tag
                elem.innerHTML += "<br>"
            }
        }
        elem.innerHTML += "Lat: " + lat + "<br>Long: " + long
        window.requestAnimationFrame(this.render_map.bind(this))
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
        let [vertices, indices] = _constructMapBuffers(this.data)
        let gl = this.gl

        gl.bindVertexArray(this.vertex_array)
        gl.bindBuffer(gl.ARRAY_BUFFER, this.vertex_buffer);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);

        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.index_buffer);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);

        gl.bindVertexArray(null)
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, null)
        gl.bindBuffer(gl.ARRAY_BUFFER, null);

        window.requestAnimationFrame(this.render_map.bind(this))
    }

    _onLeftClick(e) {
        let context_menu = document.getElementById("context-menu")
        context_menu.style.display = "none"
    }

    _onRightClick(e) {
        let context_menu = document.getElementById("context-menu")
        context_menu.style.display = "block"
        context_menu.style.left = e.pageX  + "px"
        context_menu.style.top = e.pageY + "px"
        e.preventDefault()
    }

    _setStartPos() {
        // FIXME: Ensure this runs before next pointer move
        if (this.selected_way !== null) {
            this.start_position = this.selected_way
        } else {
            this.start_position = null
        }
        window.requestAnimationFrame(this.render_map.bind(this))
    }

    _clearStartPos() {
        this.start_position = null
        this.planned_path = null
        window.requestAnimationFrame(this.render_map.bind(this))
    }

    _setSearchEnd(way_position) {
        this.planned_path = _planRoute(this.data, this.start_position, way_position, this.debug_paths)
    }
}

async function init() {
    let resp = await fetch("/data.json")
    let data = await resp.json()
    _preprocessData(data)
    let renderer = new Renderer(data)
    let pointer_tracker = new PointerTracker(renderer)
    let debug_path_button = document.getElementById("debug-path")
}


init()