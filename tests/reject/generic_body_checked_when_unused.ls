// Nothing calls `unused`, so a checker that only looked at instantiations
// would never see this. Generic bodies are checked once, rigidly.
//~ ERROR expected `int`, found `bool`

fn unused[T](x: T) -> [] int {
    return true;
}

fn main() -> [] int {
    return 0;
}
