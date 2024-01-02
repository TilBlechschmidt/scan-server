FROM alpine

COPY target/x86_64-unknown-linux-musl/release/scan-server /scan-server

EXPOSE 3030

ENTRYPOINT /scan-server