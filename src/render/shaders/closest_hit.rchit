#version 460
#extension GL_EXT_ray_tracing : require
#include "shared.glsl"
layout(location = 0) rayPayloadInEXT RayPayload payload;

void main() {
    payload.hitSky = 0.0;
}
