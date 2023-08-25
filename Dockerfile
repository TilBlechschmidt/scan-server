FROM alpine

COPY target/x86_64-unknown-linux-musl/release/server /scan-server

EXPOSE 3030

ENTRYPOINT /scan-server