```shell
docker compose run --rm rust

cd dbg_target
cargo build

cd ..
cargo run ../dbg_target/target/debug/dbg_target
```