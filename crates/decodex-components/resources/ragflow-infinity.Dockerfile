# The official ARM64 0.7.4 binary requires glibc 2.38, but its image contains 2.35.
# Keep RAGFlow's compatible binary and supply the required native runtime.
FROM infiniflow/infinity:v0.7.4-arm64@sha256:6a9e6402bb7642008dbc20367a1626da54bf0aed197fa8853560e92b9f55e801 AS upstream
FROM ubuntu:24.04@sha256:562111b88caa79c71391c0c4d3c2114a7d5c35915c20561fd893cb1e07929e6b
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl libatomic1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=upstream /usr/bin/infinity /usr/bin/infinity
COPY --from=upstream /usr/share/infinity/resource /usr/share/infinity/resource
ENTRYPOINT ["/usr/bin/infinity"]
