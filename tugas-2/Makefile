database.db: schema.sql
	python -c 'from crud import app, database; exec("with app.app_context(): database.init();")'

.PHONY: migrate

migrate: database.db
