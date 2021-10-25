#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(location = 0) rayPayloadInEXT RayPayload payload;

void main() {
    payload.didHit = false;
    payload.color = vec3(0.1, 0.1, 0.1);
}
