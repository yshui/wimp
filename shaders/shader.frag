#version 450

layout(push_constant) uniform DataBlock {
    float time;
};

layout(location = 0) in vec2 coord;
layout(location = 0) out vec4 out_color;

void main() {
    vec2 coord2 = coord * coord;
    float r2 = coord2.x + coord2.y;
    float r = sqrt(r2);
    float theta;
    float tau = asin(1);
    float radius = 0.5;
    if (time > 4.7) {
        radius = 0.5 - exp(time * 20 - 94) / exp(4) * 0.5;
    }
    if (r == 0) {
        theta = 0;
    } else {
        if (coord.y >= 0)
            theta = acos(coord.x / r);
        else
            theta = -acos(coord.x / r);
    }
    float offset = 1.331*time*time*time - 8.954*time*time + 25*time;
    theta = mod(theta - offset/4, tau / 7 * 4);
    float dist1 = r2 + radius*radius - 2*radius*r*cos(theta);
    float dist2 = r2 + radius*radius - 2*radius*r*cos(theta - tau / 7 * 4);
    float dist = min(dist1, dist2);
    if (dist <= 0.01) {
        out_color = vec4(1);
    } else {
        float u = (dist - 0.01) * 100;
        float normdist = 1 / sqrt(4 * tau) * exp(-u*u/2);
        out_color = vec4(vec3(0), normdist);
    }
    if (time < 0.4) {
        out_color *= time / 0.4;
    }
    if (time > 4.7) {
        out_color *= (5-time) / 0.3;
    }
}
