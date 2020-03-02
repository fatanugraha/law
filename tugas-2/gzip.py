from flask import Flask
import flask
import sys
import uuid
import subprocess
import shutil
import os

from utils import stream_to_file, FileTooLarge

app = Flask(__name__)

# Configurations
TEMP_DIR = os.getenv("TEMP_DIR", "/tmp")
MAX_FILE_SIZE = os.getenv("MAX_FILE_SIZE", 10 * 1024 * 1024)  # 10 MB


@app.route('/gzip', methods=["POST"])
def compress_gzip():
    filename = flask.request.args.get('filename', 'file.bin')
    file_relpath = os.path.join(str(uuid.uuid4()), filename)
    saved_path = os.path.join(TEMP_DIR, file_relpath)

    @flask.after_this_request
    def unlink_file(response):
        shutil.rmtree(os.path.dirname(saved_path))
        return response

    try:
        stream_to_file(flask.request.stream, saved_path, max_size=MAX_FILE_SIZE)
    except FileTooLarge:
        return flask.jsonify({"error": "file too large"}), 413

    process = subprocess.run(["gzip", saved_path])
    return flask.send_file(f"{saved_path}.gz")


if __name__ == '__main__':
    app.run()
