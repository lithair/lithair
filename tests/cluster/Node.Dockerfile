# Match the build's libc/toolchain image. The only executable is the test fixture.
FROM rust:1.97.1-slim@sha256:3b2879047d42784ca9403ad20c51ed3df361a50f1df96f5777d39b4e33aa65cd
COPY node /usr/local/bin/cluster-compose-node
ENTRYPOINT ["/usr/local/bin/cluster-compose-node"]
