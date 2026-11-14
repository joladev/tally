FROM rust:1.100.0-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked && mkdir /data

FROM gcr.io/distroless/cc-debian13:latest@sha256:d74d144baa124dd9defd01d71872bb56c49d4441dba6153ccf790364f97e64b1
COPY --from=build /src/target/release/tally /usr/local/bin/tally
COPY --from=build --chown=nonroot:nonroot /data /data
ENV TALLY_DATA_DIR=/data
EXPOSE 3000/tcp 5515/tcp 5514/udp
USER nonroot
ENTRYPOINT ["/usr/local/bin/tally"]
