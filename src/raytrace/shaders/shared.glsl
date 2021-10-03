struct RayPayload {
    vec3 color;
    float t;
    bool didHit;
};

struct ShadowRayPayload {
    bool shadowed;
};


struct HitAttributes {
    vec3 normal;
    uint numIterations;
};
