#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(location = 0) rayPayloadInEXT RayPayload payload;

void main() {
    payload.color = vec3(1.0, 1.0, 1.0);
    payload.t = gl_RayTmaxEXT;
}
