# The official ARM64 0.7.4 binary requires glibc 2.38, but its image contains 2.35.
# Keep RAGFlow's compatible binary and supply the required native runtime.
FROM infiniflow/infinity:v0.7.4-arm64 AS upstream
FROM ubuntu:24.04
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl libatomic1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=upstream /usr/bin/infinity /usr/bin/infinity
COPY --from=upstream /usr/share/infinity/resource /usr/share/infinity/resource
ENTRYPOINT ["/usr/bin/infinity"]
