# Stage 1: Build frontend
FROM node:20-alpine AS frontend-builder
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:1.82-alpine AS backend-builder
WORKDIR /app
RUN apk add --no-cache musl-dev pkgconf openssl-dev openssl-libs-static

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Build actual binary
COPY src/ src/
COPY migrations/ migrations/
COPY --from=frontend-builder /app/web/dist/ web/dist/
RUN touch src/main.rs && cargo build --release

# Stage 3: Runtime
FROM alpine:3.20
RUN apk add --no-cache ca-certificates
WORKDIR /app

COPY --from=backend-builder /app/target/release/tgTool /app/tgTool
COPY --from=frontend-builder /app/web/dist/ /app/web/dist/

EXPOSE 3000
ENV RUST_LOG=info
ENV PORT=3000

ENTRYPOINT ["/app/tgTool"]
