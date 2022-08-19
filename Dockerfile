# you can set --build-arg profile=release to get a release build

# create a dummy project with the same dependencies to precompile them
FROM rust:1.63 AS builder
ARG profile=dev
RUN cargo init . --name nsa
COPY Cargo.toml .
COPY Cargo.lock .
RUN cargo build --profile $profile
RUN rm -rf ./src/

# now just compile the source code when it changes
FROM builder AS installer
COPY src ./src
RUN cargo build --profile $profile

# copy the result binary to a slim image
FROM debian:buster-slim
ARG profile
# if profile isn't set, the default is dev, which outputs to the unusual path "debug"
COPY --from=installer /target/${profile:-debug}/nsa /usr/local/bin
CMD ["nsa"]