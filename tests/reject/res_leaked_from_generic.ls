//~ ERROR instantiated at `File`

// A type parameter is `val` (§3: mode is never inferred), so this body is
// accepted where it is written. Monomorphisation then checks each copy for
// real, and the copy at `File` leaks.
//
// §12 lists mode polymorphism as open. This is what it costs to get it for
// free from monomorphisation: the error is at the instantiation, not the
// definition, so the message names which one.

res struct File { fd: int }

fn open(fd: int) -> File {
    return File { fd: fd };
}

fn sink[T](x: T) -> int {
    return 0;
}

fn main() -> int {
    return sink(open(5));
}
