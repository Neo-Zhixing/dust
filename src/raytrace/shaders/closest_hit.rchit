#version 460
#extension GL_EXT_ray_tracing : require

#include "shared.glsl"

layout(location = 0) rayPayloadInEXT RayPayload payload;

void main() {
    if (gl_InstanceCustomIndexEXT == 43) {

      payload.color = vec3(1.0, 1.0, 0.0);
    } else {
        
      payload.color = vec3(1.0, 1.0, 1.0);
    }
    payload.t = gl_RayTmaxEXT;
}
