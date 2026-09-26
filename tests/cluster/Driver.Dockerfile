FROM ghcr.io/probatum-org/probatum:0.10.0@sha256:848e3900d92f3a95e61c6565be666b7ce8f13c2652a259239424057c09049afc AS probatum
FROM docker:29.7.1-cli@sha256:27a51d5ab1cd38d9eeaba7b415b8c07bc10c31e1cf1ec8d78f6413fcfab3f44f AS docker
FROM python:3.14.7-alpine3.24@sha256:4677924bcc0e94505a3270e87cb1601c2af54cd92d021b8dc306618a14333bbe
COPY --from=probatum /usr/local/bin/probatum /usr/local/bin/probatum
COPY --from=docker /usr/local/bin/docker /usr/local/bin/docker
COPY --from=docker /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/libexec/docker/cli-plugins/docker-compose
COPY checks.py native_checks.py compose.yml probatum.toml /suite/
