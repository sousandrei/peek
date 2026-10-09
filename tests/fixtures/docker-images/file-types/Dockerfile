FROM ubuntu:24.04

COPY target.txt /opt/peek/target.txt

RUN chmod 600 /opt/peek/target.txt \
    && ln -s target.txt /opt/peek/symlink \
    && ln /opt/peek/target.txt /opt/peek/hardlink
