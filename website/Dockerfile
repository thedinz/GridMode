FROM nginx:1.27-alpine

COPY website/nginx.conf /etc/nginx/conf.d/default.conf
COPY website/public/ /usr/share/nginx/html/

EXPOSE 80
