import sqlite3
import sys
import flask
import os

DATABASE_PATH = 'database.db'


def get_connection():
    db = getattr(flask.g, '_database', None)
    if db is None:
        db = flask.g._database = sqlite3.connect(DATABASE_PATH)
    return db


def query(query, *args):
    db = get_connection()
    cur = db.cursor().execute(query, args)
    rows = cur.fetchall()
    cur.close()
    return rows


def execute(query, *args):
    db = get_connection()
    cur = db.cursor().execute(query, args)
    db.commit()


def init():
    schema = os.path.join(os.path.dirname(os.path.abspath(__file__)), "schema.sql")

    db = get_connection()
    with open(schema, mode='r') as f:
        db.cursor().executescript(f.read())
    db.commit()


def close():
    db = getattr(flask.g, '_database', None)
    if db is not None:
        db.close()
