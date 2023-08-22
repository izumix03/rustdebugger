# 何をベースに始めるか
FROM rust:1.67.1

# キー=バリュー
# cargoの出力ディレクトリの指定
ENV CARGO_TARGET_DIR=/tmp/target \
  DEBIAN_FRONTEND=noninteractive \
  LC_CTYPE=ja_JP.utf8 \
  LANG=ja_JP.utf8

RUN apt-get update \
  && apt-get upgrade -y

WORKDIR /app