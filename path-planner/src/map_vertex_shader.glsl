#version 300 es

precision highp float;
precision highp int;
in vec2 long_lat;
in int way_id;
in vec3 v_color;

flat out int f_way_id;
flat out int f_selected_way;
out vec3 f_color;
uniform float scale;
uniform vec2 center;
uniform float aspect_ratio;
uniform int selected_way;

void main(void) {
	float lat_rad = long_lat.y * 3.1415962 / 180.0;
	vec2 pos = (long_lat - center) * scale;
	pos.x = pos.x * cos(lat_rad) / aspect_ratio;
	gl_Position = vec4(pos, 0.1, 1.0);
	gl_PointSize = 5.0;
	f_way_id = way_id;
	f_selected_way = selected_way;
	f_color = v_color;
}
