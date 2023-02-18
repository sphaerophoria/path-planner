#version 300 es

precision highp float;
precision highp int;
flat in int f_way_id;
flat in int f_selected_way;
in vec3 f_color;
out vec4 fragColor;

void main(void) {
	if (f_way_id == f_selected_way) {
		fragColor = vec4(1.0, 0.0, 0.0, 1.0);
	} else {
		fragColor = vec4(f_color, 1.0);
	}
}
