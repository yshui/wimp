#version 450
layout(location = 0) in vec2 v;
layout(location = 0) out vec2 coord;

void main() {
    gl_Position = vec4(v, 0, 1);
    coord = v;
}
