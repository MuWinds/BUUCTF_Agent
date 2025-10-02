# 基于Ubuntu 22.04
FROM ubuntu:22.04

# 安装基础工具和SSH服务
RUN apt-get update && \
    apt-get install -y \
        openssh-server \
        openssl \
        python3 \
        python3-pip \
        curl \
        wget \
        nmap \
        hashcat \
        john \
        sqlmap\
        sudo && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# 安装sqlmap和Python requests包
RUN pip3 install sqlmap requests pycryptodome

# 配置SSH
RUN mkdir /var/run/sshd && \
    sed -i 's/#PermitRootLogin prohibit-password/PermitRootLogin yes/' /etc/ssh/sshd_config && \
    sed -i 's/#PasswordAuthentication yes/PasswordAuthentication yes/' /etc/ssh/sshd_config && \
    sed -i 's/#Port 22/Port 22/' /etc/ssh/sshd_config

# 设置root密码为"ctfagent"，可根据需要修改
RUN echo 'root:ctfagent' | chpasswd

# 启动SSH服务
CMD ["/usr/sbin/sshd", "-D"]