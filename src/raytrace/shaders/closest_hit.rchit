#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(location = 0) rayPayloadInEXT RayPayload payload;

hitAttributeEXT float numIterations;

void main() {
    payload.color = vec3(numIterations, numIterations, numIterations);
    payload.t = gl_RayTmaxEXT;
}
