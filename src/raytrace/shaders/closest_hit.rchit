#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(set = 1, binding = 0) uniform accelerationStructureEXT accelerationStructure;
layout(location = 0) rayPayloadInEXT RayPayload payload;
layout(location = 1) rayPayloadEXT ShadowRayPayload shadowRayPayload;
hitAttributeEXT HitAttributes hitAttributes;

void main() {
    vec3 color = vec3(1, 1, 1);
    vec3 sunlight = normalize(vec3(2, -1, 2));

    float normalFactor = dot(sunlight, hitAttributes.normal);
    payload.color = color * max(0, normalFactor);

    if (normalFactor > 0) {
        shadowRayPayload.shadowed = true;
        traceRayEXT(accelerationStructure, // acceleration structure
            gl_RayFlagsOpaqueEXT | gl_RayFlagsTerminateOnFirstHitEXT | gl_RayFlagsSkipClosestHitShaderEXT,       // rayFlags
            0xFF,           // cullMask
            0,              // sbtRecordOffset
            0,              // sbtRecordStride
            1,              // missIndex
            gl_WorldRayOriginEXT + gl_HitTEXT * gl_WorldRayDirectionEXT,     // ray origin
            0.01,           // ray min range
            -sunlight,  // ray direction
            100,           // ray max range
            1               // payload (location = 0)
        );
        if (shadowRayPayload.shadowed) {
            payload.color = vec3(0,0,0);
        }
    }
    payload.color = payload.color * 0.8 + vec3(0.2, 0.2, 0.2);
}
