FROM python:3.12-alpine
RUN pip install --no-cache-dir flask requests
COPY server.py /app/server.py
WORKDIR /app
CMD ["python", "server.py"]
