import dataclasses
import sqlite3

from flask import Flask, jsonify, request

import database, repository
from utils import require_auth

app = Flask(__name__)


@app.teardown_appcontext
def close_connection(exception):
    database.close()


def serialize_document(document):
    return jsonify(dataclasses.asdict(document))


def document_not_found():
    return jsonify({"error": "document not found"})


@app.route('/documents', methods=["POST"])
@require_auth
def document_list():
    document = repository.create(request.get_json(force=True))
    return jsonify(dataclasses.asdict(document)), 201


@app.route('/documents/<uuid:key>', methods=["GET", "DELETE", "PUT"])
@require_auth
def document_detail(key):
    key = str(key)

    try:
        if request.method == "GET":
            document = repository.read(key)
            return serialize_document(document)

        elif request.method == "DELETE":
            repository.delete(key)
            return jsonify(""), 204

        elif request.method == "PUT":
            document = repository.update(key, request.get_json(force=True))
            return serialize_document(document)
    except repository.DocumentNotExist:
        return document_not_found(), 404


if __name__ == '__main__':
    app.run()
