#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(location = 0) rayPayloadInEXT RayPayload payload;

hitAttributeEXT HitAttributes hitAttributes;

void main() {
    payload.didHit = true;
    vec3 color = vec3(1, 1, 1);
    vec3 sunlight = normalize(vec3(0.3, -1, 0.7));
    float normalFactor = min(max(dot(sunlight, hitAttributes.normal), 0), 0.8) + 0.2;
    payload.color = color * normalFactor;
}
