import functools
import os

import requests
from flask import jsonify, request


class FileTooLarge(Exception):
    pass


def stream_to_file(stream, filename, max_size=None, chunk_size=4096):
    os.makedirs(os.path.dirname(filename))

    total_read = 0
    with open(filename, "bw") as f:
        while True:
            chunk = stream.read(chunk_size)
            nbytes = len(chunk)
            if nbytes == 0:
                break

            total_read += nbytes
            if max_size and total_read > max_size:
                raise FileTooLarge

            f.write(chunk)


def check_authorization(token):
    URL = "http://oauth.infralabs.cs.ui.ac.id/oauth/resource"
    data_headers = {'Authorization': token}
    r = requests.get(url=URL, headers=data_headers)
    return r.status_code == 200


def require_auth(func):
    @functools.wraps(func)
    def wrap(*args, **kwargs):
        auth_token = request.headers.get('Authorization',)
        if not auth_token or not auth_token.startswith('Bearer'):
            return jsonify({"error": "unauthorized"}), 401

        if not check_authorization(auth_token):
            return jsonify({"error": "forbidden"}), 403

        return func(*args, **kwargs)

    return wrap
