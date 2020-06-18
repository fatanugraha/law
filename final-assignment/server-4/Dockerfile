FROM debian:buster-slim

RUN apt-get update && apt-get install -y wget libpcre3-dev gcc make

WORKDIR /nginx

RUN wget http://nginx.org/download/nginx-1.18.0.tar.gz && \
    tar -xzvf nginx-1.18.0.tar.gz && \
    rm nginx-1.18.0.tar.gz && \
    cd nginx-1.18.0 && \
    ./configure --with-http_secure_link_module --conf-path=/app/nginx.conf --without-http_gzip_module && \
    make && make install

WORKDIR /app

COPY . .

CMD ["sh", "run.sh"]
