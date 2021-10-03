#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(location = 0) rayPayloadInEXT ShadowRayPayload payload;

void main() {
    payload.shadowed = false;
}
