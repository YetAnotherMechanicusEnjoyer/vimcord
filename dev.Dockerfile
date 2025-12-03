FROM archlinux/archlinux:latest

ENV DEBIAN_FRONTEND=noninteractive
ENV PACMAN_KEY_INIT=true

RUN pacman -Syu --noconfirm && \
  pacman -S base-devel rust --noconfirm

RUN useradd -m builder
WORKDIR /home/builder

COPY --chown=builder:builder . .

USER builder

RUN makepkg -sc --noconfirm

CMD [ "ls", "-l" ]
