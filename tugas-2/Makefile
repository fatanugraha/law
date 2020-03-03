database.db: schema.sql
	python -c 'from crud import app, database; exec("with app.app_context(): database.init();")'

.PHONY: migrate crud storage gzip

migrate: database.db

crud: migrate
	gunicorn --bind 0.0.0.0:20662 crud:app

storage:
	gunicorn --bind 0.0.0.0:20663 storage:app

gzip:
	gunicorn --bind 0.0.0.0:20664 gzip:app
