# select build image
FROM rust:1.43 as build

# create a new empty shell project
RUN USER=root cargo new --bin app
WORKDIR /app

# copy over your manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# this build step will cache your dependencies
RUN cargo build --release
RUN rm src/*.rs

# copy your source tree
COPY . .

# build for release
RUN rm ./target/release/deps/server_2*
RUN cargo build --release

# our final base
FROM debian:buster-slim

RUN apt-get update && apt-get install -y libssl1.1 openssl ca-certificates
# copy the build artifact from the build stage
COPY --from=build /app/target/release/server-2 .

# set the startup command to run your binary
CMD ["./server-2"]
