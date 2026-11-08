# pwm-server image (decision D30): a static musl binary on `scratch`, with
# the Mozilla CA bundle (MPL-2.0, unmodified) for Postgres TLS and a non-root
# user. No shell or package manager, so no GPL software ships in the image.
# The same image serves hosted and self-hosted installs.

FROM rust:1.99-alpine AS build
# ring (rustls' crypto) compiles C and assembly.
RUN apk add --no-cache build-base ca-certificates
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p pwm-server \
    && cp target/release/pwm-server /pwm-server \
    && echo "pwm:x:10001:10001:pwm:/:/nonexistent" > /passwd \
    && echo "pwm:x:10001:" > /group \
    && mkdir -p /var-lib-pwm

FROM scratch
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /passwd /etc/passwd
COPY --from=build /group /etc/group
COPY --from=build /pwm-server /pwm-server
# The deletions file's volume (D74 §6), writable by the server.
COPY --from=build --chown=10001:10001 /var-lib-pwm /var/lib/pwm
USER 10001:10001
ENV PWM_LISTEN=0.0.0.0:8080 \
    PWM_LOG_FORMAT=json \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/pwm-server", "healthcheck"]
ENTRYPOINT ["/pwm-server"]
