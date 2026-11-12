# Skadi's command line in a container: catalog tools, profile conversion, and dry runs in CI.
# Skadi changes the system it runs on, so the image is for inspection and automation, not for
# debloating the host.
#
#   docker build -t skadi .
#   docker run --rm skadi list --category security
#   docker run --rm -v "$PWD:/work" skadi profile convert ansible developer -o /work/site.yml

FROM rust:1.94-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --no-default-features --features tui

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 skadi
COPY --from=build /src/target/release/skadi /usr/local/bin/skadi
USER skadi
WORKDIR /work
ENTRYPOINT ["skadi"]
CMD ["--help"]
