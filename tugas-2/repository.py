import dataclasses
import json
import typing
import uuid


import database


class DocumentNotExist(Exception):
    pass


@dataclasses.dataclass
class Document:
    id: uuid.UUID
    value: typing.Any


def is_exists(key):
    result = database.query("SELECT 1 FROM documents WHERE id = ?", key)
    if not result:
        return False

    return result[0][0]


def read(key) -> Document:
    rows = database.query("SELECT value FROM documents WHERE id = ?", key)
    if not rows:
        raise DocumentNotExist

    return Document(key, json.loads(rows[0][0]))


def create(value) -> Document:
    key = uuid.uuid4()
    database.execute(
        "INSERT INTO documents (id, value) VALUES (?, ?)", str(key), json.dumps(value)
    )
    return Document(key, value)


def update(key, value):
    if not is_exists(key):
        raise DocumentNotExist

    database.execute(
        "UPDATE documents SET value = ? WHERE id = ?", json.dumps(value), key
    )
    return Document(key, value)


def delete(key):
    if not is_exists(key):
        raise DocumentNotExist

    database.execute("DELETE FROM documents WHERE id = ?", key)
