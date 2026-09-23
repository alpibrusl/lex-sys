// reverse-complement -- the same algorithm as revcomp.ls: read FASTA
// records from stdin, and for each one print its header followed by its
// sequence reversed and complemented, wrapped at 60 columns.
//
// Single-threaded and not the Benchmarks Game's own entry, which forks
// one pthread per record and reads the whole input with one `read` sized
// off a doubling buffer -- `benchmarks-game.md` §2 is why that is not
// the comparison here. The complement table is IUPAC ambiguity codes
// both ways (`AT`, `CG`, `MK`, `RY`, `WW`, `SS`, `VB`, `HD`, `N`), upper
// and lower case mapping to the same upper-case output -- which is what
// the Benchmarks Game's own reference output does, checked here byte for
// byte against `revcomp-1000.txt` (the output `fasta.c` prints at
// N=1000, per the Game's own test data).
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WIDTH 60

static char complement[128];

static void pair(char base, char comp) {
    complement[(int)base] = comp;
    complement[(int)tolower((int)base)] = comp;
}

static void init_complement(void) {
    pair('A', 'T');
    pair('C', 'G');
    pair('G', 'C');
    pair('T', 'A');
    pair('U', 'A');
    pair('M', 'K');
    pair('K', 'M');
    pair('R', 'Y');
    pair('Y', 'R');
    pair('W', 'W');
    pair('S', 'S');
    pair('V', 'B');
    pair('B', 'V');
    pair('H', 'D');
    pair('D', 'H');
    pair('N', 'N');
}

// The header line, printed as read, and the sequence that follows it
// (concatenated, newlines dropped), reversed and complemented.
static void flush_record(const char *header, size_t header_len, const char *seq, size_t seq_len) {
    fwrite(header, 1, header_len, stdout);
    putchar('\n');

    size_t col = 0;
    for (size_t k = 0; k < seq_len; k++) {
        putchar(complement[(int)seq[seq_len - 1 - k]]);
        col++;
        if (col == WIDTH) {
            putchar('\n');
            col = 0;
        }
    }
    if (col != 0) putchar('\n');
}

int main(void) {
    init_complement();

    size_t cap = 1 << 20;
    char *buf = malloc(cap);
    size_t len = 0, got;
    while ((got = fread(buf + len, 1, cap - len, stdin)) > 0) {
        len += got;
        if (len == cap) {
            cap *= 2;
            buf = realloc(buf, cap);
        }
    }

    char *header = malloc(cap);
    char *seq = malloc(cap);
    size_t header_len = 0, seq_len = 0;
    int have_record = 0;
    int in_header = 0;

    for (size_t i = 0; i < len; i++) {
        char c = buf[i];
        if (in_header) {
            if (c == '\n') {
                in_header = 0;
            } else {
                header[header_len++] = c;
            }
        } else if (c == '>') {
            if (have_record) flush_record(header, header_len, seq, seq_len);
            have_record = 1;
            header_len = 0;
            seq_len = 0;
            header[header_len++] = c;
            in_header = 1;
        } else if (c != '\n') {
            seq[seq_len++] = c;
        }
    }
    if (have_record) flush_record(header, header_len, seq, seq_len);

    free(buf);
    free(header);
    free(seq);
    return 0;
}
