#version 100
precision mediump float;

#define MAX_STOPS 32

varying vec2 v_uv;
uniform int u_gradient_type;
uniform vec2 u_direction;
uniform int u_stop_count;
uniform float u_stop_positions[MAX_STOPS];
uniform vec3 u_stop_colors[MAX_STOPS];

vec3 sample_gradient(float t) {
    if (u_stop_count <= 0) return vec3(0.0);
    if (t <= u_stop_positions[0]) return u_stop_colors[0];

    int last = u_stop_count - 1;
    if (t >= u_stop_positions[last]) return u_stop_colors[last];

    for (int i = 0; i < MAX_STOPS - 1; ++i) {
        if (i >= u_stop_count - 1) break;
        float a = u_stop_positions[i];
        float b = u_stop_positions[i + 1];
        if (t >= a && t <= b) {
            float local_t = (t - a) / max(b - a, 0.00001);
            return mix(u_stop_colors[i], u_stop_colors[i + 1], local_t);
        }
    }
    return u_stop_colors[last];
}

void main() {
    float t;

    if (u_gradient_type == 0) {
        // Centre the coordinates, project onto the requested direction,
        // then map the useful range back to 0..1.
        vec2 p = v_uv - vec2(0.5);
        float projection = dot(p, normalize(u_direction));
        t = clamp(projection + 0.5, 0.0, 1.0);
    } else {
        t = 0.0;
    }

    gl_FragColor = vec4(sample_gradient(t), 1.0);
}
