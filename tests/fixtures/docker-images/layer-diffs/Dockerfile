FROM ubuntu:24.04

COPY modified.txt /opt/peek/modified.txt
COPY removed.txt /opt/peek/removed.txt
COPY replaced/ /opt/peek/replaced/

RUN printf 'updated\n' > /opt/peek/modified.txt \
    && rm /opt/peek/removed.txt \
    && rm -rf /opt/peek/replaced \
    && printf 'replacement file\n' > /opt/peek/replaced

COPY added.txt /opt/peek/added.txt
