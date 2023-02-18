#version 300 es

precision highp float;
precision highp int;
flat in int f_way_id;
flat in int f_selected_way;
in vec3 f_color;
out int fragColor;

void main(void) {
	fragColor = f_way_id;
}
