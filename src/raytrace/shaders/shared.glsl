struct RayPayload {
    vec3 color;
    float t;
    bool didHit;
};

struct HitAttributes {
    vec3 normal;
    uint numIterations;
};
