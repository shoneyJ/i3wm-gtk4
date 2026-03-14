# Build & Development

## Reference Project

For Rust + GTK4 syntax reference:

```bash
ls ~/projects/rust-gtk4-todo-app/
```

## Docker Dev Environment

```bash
# Build the container (first time only)
docker compose build

# Enter dev shell
docker compose run --rm dev

# Inside the container
cargo build --release
```

## Copy Release Binary to Host

```bash
# Inside the container, after cargo build --release
cp target/release/i3more /app/dist/
```

The `./dist` directory is bind-mounted from the host, so the binary is immediately available at `./dist/i3more`.

## Test the Binary

```bash
# On the host
./dist/i3more
```
