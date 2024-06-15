# 🦉TRSO-MIGRATOR

TRSO migrator is a simple cli application which applies SQL files from a folder set in the environment variables to the local or remote Turso database.

The main objective of the CLI is to be able to quickly work on your hobby projects. It was created purely for my personal needs, but feel free to use and send PRs.

The CLI on the first run creates `migrations` table and writes there applied migrations files, the files are going to run by the alphabetical order of the filename. 

### Enviromental Variables to set before running

| Name                   | Default Value                       | Description                                  |
| ---------------------- | ----------------------------------- | -------------------------------------------- |
| `TRSO_LOCAL`           | -                                   | Local database or remote flag                |
| `TRSO_PATH_URL`        | -                                   | File path or the url of the remote           |
| `TRSO_TOKEN`           | -                                   | Must be set if `TRSO_LOCAL` is true          |
| `TRSO_MIGRATIONS_PATH` | `<CURRENT_WORKING_DIR>`/migrations/ | Folder where the migration files are located |

