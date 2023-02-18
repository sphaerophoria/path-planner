const wasm = import('./rust/pkg')

"use strict"

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

function _contextMenuVisible() {
    let context_menu = document.getElementById("context-menu")
    return window.getComputedStyle(context_menu).display == "block"
}

class InputHandler {
    constructor(app) {
        this.app = app
        this.path_debugging = false

        this.down_pointers = []

        let canvas_holder = document.getElementById('canvas-holder')
        canvas_holder.addEventListener('pointerdown', this._onPointerDown.bind(this))
        canvas_holder.addEventListener('pointermove', this._onPointerMove.bind(this))
        canvas_holder.addEventListener('pointerup', this._onPointerUp.bind(this))

        canvas_holder.addEventListener('wheel', this._onScroll.bind(this))
        canvas_holder.onclick = this._onLeftClick.bind(this)
        canvas_holder.oncontextmenu = this._onRightClick.bind(this)

        document.getElementById('start-route').onclick = this._setStartPos.bind(this)
        document.getElementById('clear-route').onclick = this._clearStartPos.bind(this)
        document.getElementById('debug-path').onclick = () => { 
            this.debug_paths = !this.debug_paths 
            this.app.set_debug_mode(this.debug_paths)
        }

        document.getElementById('custom-highlight-regex').addEventListener('input', this._onCustomHighlightChanged.bind(this))
        document.getElementById('custom-highlight-color').addEventListener('input', this._onCustomHighlightChanged.bind(this))
    }

    _calculatePointerDistance(e1, e2) {
        let y_dist = e2.pageY - e1.pageY
        let y_dist_2 = y_dist * y_dist

        let x_dist = e2.pageX - e1.pageX
        let x_dist_2 = x_dist * x_dist
        return Math.sqrt(x_dist_2 + y_dist_2)
    }

    _onCustomHighlightChanged() {
        let regex = document.getElementById("custom-highlight-regex").value
        let color = _getRGB(document.getElementById("custom-highlight-color").value)
        this.app.update_highlight(regex, color);
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

    _onScroll(e) {
        this.app.zoom(Math.pow(1.001, -e.deltaY), e.pageX, e.pageY)
        window.requestAnimationFrame(this.app.render.bind(this.app))
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

        this.app.update_pointer_pos(e.pageX, e.pageY)

        if (this.down_pointers.length == 1) {
            let last_event = this.down_pointers[idx]
            this.down_pointers[idx] = e

            this.app.move_map(last_event.pageX - e.pageX, last_event.pageY - e.pageY)
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
            this.app.updateScale(e.pageX, e.pageY, delta)
        }

        let elem = document.getElementById("pointer-lat-long")

        elem.innerHTML = ""

        for (let tag of this.app.selected_tags()) {
            elem.innerHTML += tag
            elem.innerHTML += "<br>"
        }

        let [long, lat] = this.app.pixel_to_geocoord(e.pageX, e.pageY)
        elem.innerHTML += "Lat: " + lat + "<br>Long: " + long

        window.requestAnimationFrame(this.app.render.bind(this.app))
    }

    _onPointerDown(e) {
        let idx = this._pointerIdx(e)
        this.down_pointers[idx] = e
    }

    _pointerIdx(e) {
        let idx = this.down_pointers.findIndex((cached_ev) => cached_ev.pointerId == e.pointerId)
        if (idx == -1) {
            return this.down_pointers.length
        }

        return idx
    }

    _setStartPos(e) {
        this.app.start_path_plan()
    }

    _clearStartPos(e) {
        this.app.clear_path_plan()
    }
}

async function init() {
    let resp = await fetch("/data.json")
    let data = await resp.json()

    let m = await wasm;
    m.init();

    let app = new m.App(document.getElementById('canvas'), data);
    let input_handler = new InputHandler(app)
    app.render()
}


init()
