// fasta, from the Computer Language Benchmarks Game.
//
// `docs/benchmarks-game.md` §2.1 parked this program on network access to
// the Game's own spec, which is now reachable. Two shapes, and nothing
// else. `repeat_fasta` cycles a fixed 287-byte string, wrapped at 60
// columns. `random_fasta` draws one linear-congruential step per output
// byte (IM=139968, IA=3877, IC=29573, seed 42) and finds its symbol by a
// linear search over cumulative probabilities -- exactly the two things
// the benchmark's own description forbids optimising away ("Please
// don't optimize the cumulative-probabilities lookup ... or naive LCG
// arithmetic"). The seed is one LCG, threaded through both calls to
// `random_fasta` by its return value, because the benchmark's generator
// is a single stream and not one per call.
//
// Checked against the Benchmarks Game's own N=1000 reference output,
// byte for byte (`crates/lex-sys/tests/conformance/benchmarks.rs`,
// `benches/game/fasta-1000.txt`). `fasta.c` is the same algorithm in
// `double`, which is what lex-sys's one float type is
// (`docs/floating-point.md`) -- not the Game's own hand-tuned C entries,
// one of which precomputes a 139968-entry lookup table and the other of
// which threads across records, for the reason `docs/benchmarks-game.md`
// §2 already gives.
//
// `docs/bulk-io.md` §2.1 named this program as the open question its own
// prediction should not have made: each byte still costs a congruential
// step and a table lookup, so the write side is one `io.write_all` per
// 60-byte line rather than one `putchar` per byte, and what that is
// worth is `docs/benchmarks-game.md`'s to measure.
//
//~ STDOUT >ONE Homo sapiens alu
//~ STDOUT GGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGA
//~ STDOUT TCACCTGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACT
//~ STDOUT AAAAATACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAG
//~ STDOUT GCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCG
//~ STDOUT CCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAAGGCCGGGCGCGGT
//~ STDOUT GGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCA
//~ STDOUT GGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAA
//~ STDOUT TTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAG
//~ STDOUT AATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCA
//~ STDOUT GCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAAGGCCGGGCGCGGTGGCTCACGCCTGT
//~ STDOUT AATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGTTCGAGACC
//~ STDOUT AGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGCCGGGCGTG
//~ STDOUT GTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACC
//~ STDOUT CGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTGGGCGACAG
//~ STDOUT AGCGAGACTCCGTCTCAAAAAGGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTT
//~ STDOUT TGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACA
//~ STDOUT TGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCT
//~ STDOUT GTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGG
//~ STDOUT TTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGT
//~ STDOUT CTCAAAAAGGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGG
//~ STDOUT CGGGCGGATCACCTGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCG
//~ STDOUT TCTCTACTAAAAATACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTA
//~ STDOUT CTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCG
//~ STDOUT AGATCGCGCCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAAGGCCG
//~ STDOUT GGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACC
//~ STDOUT TGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAA
//~ STDOUT TACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGA
//~ STDOUT GGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACT
//~ STDOUT GCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAAGGCCGGGCGCGGTGGCTC
//~ STDOUT ACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGT
//~ STDOUT TCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGC
//~ STDOUT CGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCG
//~ STDOUT CTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTG
//~ STDOUT GGCGACAGAGCGAGACTCCG
//~ STDOUT >TWO IUB ambiguity codes
//~ STDOUT cttBtatcatatgctaKggNcataaaSatgtaaaDcDRtBggDtctttataattcBgtcg
//~ STDOUT tactDtDagcctatttSVHtHttKtgtHMaSattgWaHKHttttagacatWatgtRgaaa
//~ STDOUT NtactMcSMtYtcMgRtacttctWBacgaaatatagScDtttgaagacacatagtVgYgt
//~ STDOUT cattHWtMMWcStgttaggKtSgaYaaccWStcgBttgcgaMttBYatcWtgacaYcaga
//~ STDOUT gtaBDtRacttttcWatMttDBcatWtatcttactaBgaYtcttgttttttttYaaScYa
//~ STDOUT HgtgttNtSatcMtcVaaaStccRcctDaataataStcYtRDSaMtDttgttSagtRRca
//~ STDOUT tttHatSttMtWgtcgtatSSagactYaaattcaMtWatttaSgYttaRgKaRtccactt
//~ STDOUT tattRggaMcDaWaWagttttgacatgttctacaaaRaatataataaMttcgDacgaSSt
//~ STDOUT acaStYRctVaNMtMgtaggcKatcttttattaaaaagVWaHKYagtttttatttaacct
//~ STDOUT tacgtVtcVaattVMBcttaMtttaStgacttagattWWacVtgWYagWVRctDattBYt
//~ STDOUT gtttaagaagattattgacVatMaacattVctgtBSgaVtgWWggaKHaatKWcBScSWa
//~ STDOUT accRVacacaaactaccScattRatatKVtactatatttHttaagtttSKtRtacaaagt
//~ STDOUT RDttcaaaaWgcacatWaDgtDKacgaacaattacaRNWaatHtttStgttattaaMtgt
//~ STDOUT tgDcgtMgcatBtgcttcgcgaDWgagctgcgaggggVtaaScNatttacttaatgacag
//~ STDOUT cccccacatYScaMgtaggtYaNgttctgaMaacNaMRaacaaacaKctacatagYWctg
//~ STDOUT ttWaaataaaataRattagHacacaagcgKatacBttRttaagtatttccgatctHSaat
//~ STDOUT actcNttMaagtattMtgRtgaMgcataatHcMtaBSaRattagttgatHtMttaaKagg
//~ STDOUT YtaaBataSaVatactWtataVWgKgttaaaacagtgcgRatatacatVtHRtVYataSa
//~ STDOUT KtWaStVcNKHKttactatccctcatgWHatWaRcttactaggatctataDtDHBttata
//~ STDOUT aaaHgtacVtagaYttYaKcctattcttcttaataNDaaggaaaDYgcggctaaWSctBa
//~ STDOUT aNtgctggMBaKctaMVKagBaactaWaDaMaccYVtNtaHtVWtKgRtcaaNtYaNacg
//~ STDOUT gtttNattgVtttctgtBaWgtaattcaagtcaVWtactNggattctttaYtaaagccgc
//~ STDOUT tcttagHVggaYtgtNcDaVagctctctKgacgtatagYcctRYHDtgBattDaaDgccK
//~ STDOUT tcHaaStttMcctagtattgcRgWBaVatHaaaataYtgtttagMDMRtaataaggatMt
//~ STDOUT ttctWgtNtgtgaaaaMaatatRtttMtDgHHtgtcattttcWattRSHcVagaagtacg
//~ STDOUT ggtaKVattKYagactNaatgtttgKMMgYNtcccgSKttctaStatatNVataYHgtNa
//~ STDOUT BKRgNacaactgatttcctttaNcgatttctctataScaHtataRagtcRVttacDSDtt
//~ STDOUT aRtSatacHgtSKacYagttMHtWataggatgactNtatSaNctataVtttRNKtgRacc
//~ STDOUT tttYtatgttactttttcctttaaacatacaHactMacacggtWataMtBVacRaSaatc
//~ STDOUT cgtaBVttccagccBcttaRKtgtgcctttttRtgtcagcRttKtaaacKtaaatctcac
//~ STDOUT aattgcaNtSBaaccgggttattaaBcKatDagttactcttcattVtttHaaggctKKga
//~ STDOUT tacatcBggScagtVcacattttgaHaDSgHatRMaHWggtatatRgccDttcgtatcga
//~ STDOUT aacaHtaagttaRatgaVacttagattVKtaaYttaaatcaNatccRttRRaMScNaaaD
//~ STDOUT gttVHWgtcHaaHgacVaWtgttScactaagSgttatcttagggDtaccagWattWtRtg
//~ STDOUT ttHWHacgattBtgVcaYatcggttgagKcWtKKcaVtgaYgWctgYggVctgtHgaNcV
//~ STDOUT taBtWaaYatcDRaaRtSctgaHaYRttagatMatgcatttNattaDttaattgttctaa
//~ STDOUT ccctcccctagaWBtttHtBccttagaVaatMcBHagaVcWcagBVttcBtaYMccagat
//~ STDOUT gaaaaHctctaacgttagNWRtcggattNatcRaNHttcagtKttttgWatWttcSaNgg
//~ STDOUT gaWtactKKMaacatKatacNattgctWtatctaVgagctatgtRaHtYcWcttagccaa
//~ STDOUT tYttWttaWSSttaHcaaaaagVacVgtaVaRMgattaVcDactttcHHggHRtgNcctt
//~ STDOUT tYatcatKgctcctctatVcaaaaKaaaagtatatctgMtWtaaaacaStttMtcgactt
//~ STDOUT taSatcgDataaactaaacaagtaaVctaggaSccaatMVtaaSKNVattttgHccatca
//~ STDOUT cBVctgcaVatVttRtactgtVcaattHgtaaattaaattttYtatattaaRSgYtgBag
//~ STDOUT aHSBDgtagcacRHtYcBgtcacttacactaYcgctWtattgSHtSatcataaatataHt
//~ STDOUT cgtYaaMNgBaatttaRgaMaatatttBtttaaaHHKaatctgatWatYaacttMctctt
//~ STDOUT ttVctagctDaaagtaVaKaKRtaacBgtatccaaccactHHaagaagaaggaNaaatBW
//~ STDOUT attccgStaMSaMatBttgcatgRSacgttVVtaaDMtcSgVatWcaSatcttttVatag
//~ STDOUT ttactttacgatcaccNtaDVgSRcgVcgtgaacgaNtaNatatagtHtMgtHcMtagaa
//~ STDOUT attBgtataRaaaacaYKgtRccYtatgaagtaataKgtaaMttgaaRVatgcagaKStc
//~ STDOUT tHNaaatctBBtcttaYaBWHgtVtgacagcaRcataWctcaBcYacYgatDgtDHccta
//~ STDOUT >THREE Homo sapiens frequency
//~ STDOUT aacacttcaccaggtatcgtgaaggctcaagattacccagagaacctttgcaatataaga
//~ STDOUT atatgtatgcagcattaccctaagtaattatattctttttctgactcaaagtgacaagcc
//~ STDOUT ctagtgtatattaaatcggtatatttgggaaattcctcaaactatcctaatcaggtagcc
//~ STDOUT atgaaagtgatcaaaaaagttcgtacttataccatacatgaattctggccaagtaaaaaa
//~ STDOUT tagattgcgcaaaattcgtaccttaagtctctcgccaagatattaggatcctattactca
//~ STDOUT tatcgtgtttttctttattgccgccatccccggagtatctcacccatccttctcttaaag
//~ STDOUT gcctaatattacctatgcaaataaacatatattgttgaaaattgagaacctgatcgtgat
//~ STDOUT tcttatgtgtaccatatgtatagtaatcacgcgactatatagtgctttagtatcgcccgt
//~ STDOUT gggtgagtgaatattctgggctagcgtgagatagtttcttgtcctaatatttttcagatc
//~ STDOUT gaatagcttctatttttgtgtttattgacatatgtcgaaactccttactcagtgaaagtc
//~ STDOUT atgaccagatccacgaacaatcttcggaatcagtctcgttttacggcggaatcttgagtc
//~ STDOUT taacttatatcccgtcgcttactttctaacaccccttatgtatttttaaaattacgttta
//~ STDOUT ttcgaacgtacttggcggaagcgttattttttgaagtaagttacattgggcagactcttg
//~ STDOUT acattttcgatacgactttctttcatccatcacaggactcgttcgtattgatatcagaag
//~ STDOUT ctcgtgatgattagttgtcttctttaccaatactttgaggcctattctgcgaaatttttg
//~ STDOUT ttgccctgcgaacttcacataccaaggaacacctcgcaacatgccttcatatccatcgtt
//~ STDOUT cattgtaattcttacacaatgaatcctaagtaattacatccctgcgtaaaagatggtagg
//~ STDOUT ggcactgaggatatattaccaagcatttagttatgagtaatcagcaatgtttcttgtatt
//~ STDOUT aagttctctaaaatagttacatcgtaatgttatctcgggttccgcgaataaacgagatag
//~ STDOUT attcattatatatggccctaagcaaaaacctcctcgtattctgttggtaattagaatcac
//~ STDOUT acaatacgggttgagatattaattatttgtagtacgaagagatataaaaagatgaacaat
//~ STDOUT tactcaagtcaagatgtatacgggatttataataaaaatcgggtagagatctgctttgca
//~ STDOUT attcagacgtgccactaaatcgtaatatgtcgcgttacatcagaaagggtaactattatt
//~ STDOUT aattaataaagggcttaatcactacatattagatcttatccgatagtcttatctattcgt
//~ STDOUT tgtatttttaagcggttctaattcagtcattatatcagtgctccgagttctttattattg
//~ STDOUT ttttaaggatgacaaaatgcctcttgttataacgctgggagaagcagactaagagtcgga
//~ STDOUT gcagttggtagaatgaggctgcaaaagacggtctcgacgaatggacagactttactaaac
//~ STDOUT caatgaaagacagaagtagagcaaagtctgaagtggtatcagcttaattatgacaaccct
//~ STDOUT taatacttccctttcgccgaatactggcgtggaaaggttttaaaagtcgaagtagttaga
//~ STDOUT ggcatctctcgctcataaataggtagactactcgcaatccaatgtgactatgtaatactg
//~ STDOUT ggaacatcagtccgcgatgcagcgtgtttatcaaccgtccccactcgcctggggagacat
//~ STDOUT gagaccacccccgtggggattattagtccgcagtaatcgactcttgacaatccttttcga
//~ STDOUT ttatgtcatagcaatttacgacagttcagcgaagtgactactcggcgaaatggtattact
//~ STDOUT aaagcattcgaacccacatgaatgtgattcttggcaatttctaatccactaaagcttttc
//~ STDOUT cgttgaatctggttgtagatatttatataagttcactaattaagatcacggtagtatatt
//~ STDOUT gatagtgatgtctttgcaagaggttggccgaggaatttacggattctctattgatacaat
//~ STDOUT ttgtctggcttataactcttaaggctgaaccaggcgtttttagacgacttgatcagctgt
//~ STDOUT tagaatggtttggactccctctttcatgtcagtaacatttcagccgttattgttacgata
//~ STDOUT tgcttgaacaatattgatctaccacacacccatagtatattttataggtcatgctgttac
//~ STDOUT ctacgagcatggtattccacttcccattcaatgagtattcaacatcactagcctcagaga
//~ STDOUT tgatgacccacctctaataacgtcacgttgcggccatgtgaaacctgaacttgagtagac
//~ STDOUT gatatcaagcgctttaaattgcatataacatttgagggtaaagctaagcggatgctttat
//~ STDOUT ataatcaatactcaataataagatttgattgcattttagagttatgacacgacatagttc
//~ STDOUT actaacgagttactattcccagatctagactgaagtactgatcgagacgatccttacgtc
//~ STDOUT gatgatcgttagttatcgacttaggtcgggtctctagcggtattggtacttaaccggaca
//~ STDOUT ctatactaataacccatgatcaaagcataacagaatacagacgataatttcgccaacata
//~ STDOUT tatgtacagaccccaagcatgagaagctcattgaaagctatcattgaagtcccgctcaca
//~ STDOUT atgtgtcttttccagacggtttaactggttcccgggagtcctggagtttcgacttacata
//~ STDOUT aatggaaacaatgtattttgctaatttatctatagcgtcatttggaccaatacagaatat
//~ STDOUT tatgttgcctagtaatccactataacccgcaagtgctgatagaaaatttttagacgattt
//~ STDOUT ataaatgccccaagtatccctcccgtgaatcctccgttatactaattagtattcgttcat
//~ STDOUT acgtataccgcgcatatatgaacatttggcgataaggcgcgtgaattgttacgtgacaga
//~ STDOUT gatagcagtttcttgtgatatggttaacagacgtacatgaagggaaactttatatctata
//~ STDOUT gtgatgcttccgtagaaataccgccactggtctgccaatgatgaagtatgtagctttagg
//~ STDOUT tttgtactatgaggctttcgtttgtttgcagagtataacagttgcgagtgaaaaaccgac
//~ STDOUT gaatttatactaatacgctttcactattggctacaaaatagggaagagtttcaatcatga
//~ STDOUT gagggagtatatggatgctttgtagctaaaggtagaacgtatgtatatgctgccgttcat
//~ STDOUT tcttgaaagatacataagcgataagttacgacaattataagcaacatccctaccttcgta
//~ STDOUT acgatttcactgttactgcgcttgaaatacactatggggctattggcggagagaagcaga
//~ STDOUT tcgcgccgagcatatacgagacctataatgttgatgatagagaaggcgtctgaattgata
//~ STDOUT catcgaagtacactttctttcgtagtatctctcgtcctctttctatctccggacacaaga
//~ STDOUT attaagttatatatatagagtcttaccaatcatgttgaatcctgattctcagagttcttt
//~ STDOUT ggcgggccttgtgatgactgagaaacaatgcaatattgctccaaatttcctaagcaaatt
//~ STDOUT ctcggttatgttatgttatcagcaaagcgttacgttatgttatttaaatctggaatgacg
//~ STDOUT gagcgaagttcttatgtcggtgtgggaataattcttttgaagacagcactccttaaataa
//~ STDOUT tatcgctccgtgtttgtatttatcgaatgggtctgtaaccttgcacaagcaaatcggtgg
//~ STDOUT tgtatatatcggataacaattaatacgatgttcatagtgacagtatactgatcgagtcct
//~ STDOUT ctaaagtcaattacctcacttaacaatctcattgatgttgtgtcattcccggtatcgccc
//~ STDOUT gtagtatgtgctctgattgaccgagtgtgaaccaaggaacatctactaatgcctttgtta
//~ STDOUT ggtaagatctctctgaattccttcgtgccaacttaaaacattatcaaaatttcttctact
//~ STDOUT tggattaactacttttacgagcatggcaaattcccctgtggaagacggttcattattatc
//~ STDOUT ggaaaccttatagaaattgcgtgttgactgaaattagatttttattgtaagagttgcatc
//~ STDOUT tttgcgattcctctggtctagcttccaatgaacagtcctcccttctattcgacatcgggt
//~ STDOUT ccttcgtacatgtctttgcgatgtaataattaggttcggagtgtggccttaatgggtgca
//~ STDOUT actaggaatacaacgcaaatttgctgacatgatagcaaatcggtatgccggcaccaaaac
//~ STDOUT gtgctccttgcttagcttgtgaatgagactcagtagttaaataaatccatatctgcaatc
//~ STDOUT gattccacaggtattgtccactatctttgaactactctaagagatacaagcttagctgag
//~ STDOUT accgaggtgtatatgactacgctgatatctgtaaggtaccaatgcaggcaaagtatgcga
//~ STDOUT gaagctaataccggctgtttccagctttataagattaaaatttggctgtcctggcggcct
//~ STDOUT cagaattgttctatcgtaatcagttggttcattaattagctaagtacgaggtacaactta
//~ STDOUT tctgtcccagaacagctccacaagtttttttacagccgaaacccctgtgtgaatcttaat
//~ STDOUT atccaagcgcgttatctgattagagtttacaactcagtattttatcagtacgttttgttt
//~ STDOUT ccaacattacccggtatgacaaaatgacgccacgtgtcgaataatggtctgaccaatgta
//~ STDOUT ggaagtgaaaagataaatat
//~ EXIT 0

import std.bytes;
import std.io;

fn repeat_fasta[&i, &s](out: &!i Io, seq: &s [byte], total: int) -> [io_write] int {
    let seqlen = len(seq);
    region a {
        let line = alloc_slice[a](61, byte_of(0));
        line[60] = byte_of('\n');

        var pos = 0;
        var remaining = total;
        while remaining > 0 {
            var take = 60;
            if remaining < 60 {
                take = remaining;
            }
            var k = 0;
            while k < take {
                line[k] = seq[(pos + k) % seqlen];
                k = k + 1;
            }
            if take == 60 {
                io.write_all(out, line);
            } else {
                io.write_all(out, line[0..take]);
                io.newline(out);
            }
            pos = (pos + take) % seqlen;
            remaining = remaining - take;
        }
    }
    return 0;
}

// Fills `probabilities` in place with its own running sum, turning a
// list of weights into the cumulative table `random_fasta` searches.
fn accumulate[&p](probabilities: &!p [float]) -> [] int {
    var sum = 0.0;
    var i = 0;
    while i < len(probabilities) {
        sum = sum + probabilities[i];
        probabilities[i] = sum;
        i = i + 1;
    }
    return 0;
}

// Draws `total` symbols from `symbols`, weighted by `cumulative`
// (already run through `accumulate`), and returns the LCG seed after the
// last draw so the next call continues the same stream.
fn random_fasta[&i, &s, &p](
    out: &!i Io,
    symbols: &s [byte],
    cumulative: &p [float],
    total: int,
    seed_in: int,
) -> [io_write] int {
    let im = 139968;
    let im_f = float_of(im);
    var seed = seed_in;
    region a {
        let line = alloc_slice[a](61, byte_of(0));
        line[60] = byte_of('\n');

        var col = 0;
        var k = 0;
        while k < total {
            seed = (seed * 3877 + 29573) % im;
            let r = float_of(seed) / im_f;
            var idx = 0;
            while cumulative[idx] < r {
                idx = idx + 1;
            }
            line[col] = symbols[idx];
            col = col + 1;
            if col == 60 {
                io.write_all(out, line);
                col = 0;
            }
            k = k + 1;
        }
        if col != 0 {
            io.write_all(out, line[0..col]);
            io.newline(out);
        }
    }
    return seed;
}

// The benchmark's `N`, from the command line, with the verified default
// this file's header states. Copied from `spectral.ls` and `fannkuch.ls`
// rather than shared, per `docs/standard-library.md` §3.1's bar of two
// askers -- this is the third, and a whole-number parser is now three
// programs writing the same four lines.
fn size_from[&g](args: &g Args, fallback: int) -> [args] int {
    if arg_count(args) < 2 {
        return fallback;
    }
    let text = arg(args, 1);
    var value = 0;
    var i = 0;
    while i < len(text) {
        let digit = bytes.digit_of(int_of(text[i]));
        if digit < 0 {
            return fallback;
        }
        value = value * 10 + digit;
        i = i + 1;
    }
    if value <= 0 {
        return fallback;
    }
    return value;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(heap);
    release(fs);
    release(ffi);

    var n = 1000;
    borrow args as &g in {
        n = size_from(g, 1000);
    }
    release(args);

    borrow mut io as &!i in {
        io.write_all(i, ">ONE Homo sapiens alu\n");
        let alu =
            "GGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAA";
        repeat_fasta(i, alu, n * 2);

        io.write_all(i, ">TWO IUB ambiguity codes\n");
        var seed = 42;
        region a {
            let iub_p = alloc_slice[a](15, 0.0);
            iub_p[0] = 0.27;
            iub_p[1] = 0.12;
            iub_p[2] = 0.12;
            iub_p[3] = 0.27;
            iub_p[4] = 0.02;
            iub_p[5] = 0.02;
            iub_p[6] = 0.02;
            iub_p[7] = 0.02;
            iub_p[8] = 0.02;
            iub_p[9] = 0.02;
            iub_p[10] = 0.02;
            iub_p[11] = 0.02;
            iub_p[12] = 0.02;
            iub_p[13] = 0.02;
            iub_p[14] = 0.02;
            accumulate(iub_p);
            seed = random_fasta(i, "acgtBDHKMNRSVWY", iub_p, n * 3, seed);
        }

        io.write_all(i, ">THREE Homo sapiens frequency\n");
        region a {
            let homo_p = alloc_slice[a](4, 0.0);
            homo_p[0] = 0.3029549426680;
            homo_p[1] = 0.1979883004921;
            homo_p[2] = 0.1975473066391;
            homo_p[3] = 0.3015094502008;
            accumulate(homo_p);
            seed = random_fasta(i, "acgt", homo_p, n * 5, seed);
        }
    }
    release(io);
    return 0;
}
