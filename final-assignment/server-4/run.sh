sed -E s/SECURE_LINK_SECRET/$SECURE_LINK_SECRET/ nginx.conf -i
/usr/local/nginx/sbin/nginx -g 'daemon off;'
