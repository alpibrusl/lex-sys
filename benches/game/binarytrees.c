// binary-trees — the same algorithm as binarytrees.ls.
//
// `malloc` and `free` per node, matching `box`/`unbox`: the Benchmarks
// Game's own C entry uses an arena allocator, which measures a
// different program. §2 of `benchmarks-game.md` is the rule being
// followed.
#include <stdio.h>
#include <stdlib.h>

typedef struct node { struct node *left, *right; } node;

static node *build(int depth) {
    node *t = malloc(sizeof(node));
    if (depth <= 0) { t->left = NULL; t->right = NULL; return t; }
    t->left = build(depth - 1);
    t->right = build(depth - 1);
    return t;
}

static long check_and_free(node *t) {
    if (t->left == NULL) { free(t); return 1; }
    long total = 1 + check_and_free(t->left) + check_and_free(t->right);
    free(t);
    return total;
}

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 10;
    int mindepth = 4;
    int maxdepth = n > mindepth + 2 ? n : mindepth + 2;
    int stretch = maxdepth + 1;

    printf("stretch tree of depth %d\t check: %ld\n", stretch, check_and_free(build(stretch)));
    node *longlived = build(maxdepth);

    for (int depth = mindepth; depth <= maxdepth; depth += 2) {
        long iterations = 1L << (maxdepth - depth + mindepth);
        long total = 0;
        for (long round = 0; round < iterations; round++) total += check_and_free(build(depth));
        printf("%ld\t trees of depth %d\t check: %ld\n", iterations, depth, total);
    }
    printf("long lived tree of depth %d\t check: %ld\n", maxdepth, check_and_free(longlived));
    return 0;
}
