FROM rust:1.75

WORKDIR /usr/src/app

COPY . .

RUN apt-get update -y && apt-get install -y  libsqlite3-dev
RUN cargo build --release

#inter-container communication or something idk
EXPOSE 8080

ENTRYPOINT ./target/release/kakimail "0.0.0.0"
