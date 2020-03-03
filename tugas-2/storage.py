import os
import uuid

import flask
from flask import Flask

from utils import FileTooLarge, stream_to_file, require_auth

app = Flask(__name__)

# Configurations
STORAGE_DIR = os.getenv("STORAGE_DIR", "storage")
MAX_FILE_SIZE = os.getenv("MAX_FILE_SIZE", 10 * 1024 * 1024)  # 10 MB


@app.route('/files', methods=["PUT"])
@require_auth
def store_file():
    filename = flask.request.args.get('filename')
    if not filename:
        return flask.jsonify({"error": "filename is required"}), 400

    file_relpath = os.path.join(str(uuid.uuid4()), filename)
    saved_path = os.path.join(STORAGE_DIR, file_relpath)
    try:
        stream_to_file(
            flask.request.stream, saved_path, max_size=MAX_FILE_SIZE,
        )
    except FileTooLarge:
        return flask.jsonify({"error": "file too large"}), 413

    return flask.jsonify({"url": f"/files/{file_relpath}"})


@app.route('/files/<path:path>')
@require_auth
def send_js(path):
    return flask.send_from_directory(STORAGE_DIR, path)


if __name__ == '__main__':
    app.run()
