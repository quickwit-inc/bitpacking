macro_rules! pack_unpack_with_bits {
    ($name:ident, $n:literal, $cycle:literal, $cpufeature:meta) => {
        mod $name {

            use super::BLOCK_LEN;
            use super::{
                left_shift_32, load_unaligned, op_and, op_or, right_shift_32, set1,
                store_unaligned, DataType,
            };
            use super::{Sink, Transformer};
            use unroll::unroll_for_loops;

            const NUM_BITS: usize = $n;
            const NUM_BYTES_PER_BLOCK: usize = NUM_BITS * BLOCK_LEN / 8;

            const REPEAT: usize = 32 / $cycle;

            #[$cpufeature]
            #[unroll_for_loops]
            pub(crate) unsafe fn pack<TDeltaComputer: Transformer>(
                input_arr: &[u32],
                output_arr: &mut [u8],
                mut delta_computer: TDeltaComputer,
            ) -> usize {
                assert_eq!(
                    input_arr.len(),
                    BLOCK_LEN,
                    "Input block too small {}, (expected {})",
                    input_arr.len(),
                    BLOCK_LEN
                );
                assert!(
                    output_arr.len() >= NUM_BYTES_PER_BLOCK,
                    "Output array too small (numbits {}). {} <= {}",
                    NUM_BITS,
                    output_arr.len(),
                    NUM_BYTES_PER_BLOCK
                );

                let mut input_ptr = input_arr.as_ptr() as *const DataType;
                let mut output_ptr = output_arr.as_mut_ptr() as *mut DataType;

                for _ in 0..REPEAT {
                    let mut out_register: DataType =
                        delta_computer.transform(load_unaligned(input_ptr));
                    input_ptr = input_ptr.add(1);

                    // Ideally we would have liked to write `for input_id in 1..$cycle-1`
                    // here, unfortunately unroll_for_loops needs a literal here to kick
                    // in.
                    for iter in 2..$cycle {
                        const BITS_FILLED: usize = (iter - 1) * NUM_BITS;
                        const INNER_CURSOR: usize = BITS_FILLED % 32;
                        const REMAINING: usize = 32 - INNER_CURSOR;

                        let in_register: DataType =
                            delta_computer.transform(load_unaligned(input_ptr));
                        input_ptr = input_ptr.add(1);

                        out_register = if INNER_CURSOR > 0 {
                            let shifted = left_shift_32(in_register, INNER_CURSOR as i32);
                            op_or(out_register, shifted)
                        } else {
                            in_register
                        };

                        if REMAINING <= NUM_BITS {
                            store_unaligned(output_ptr, out_register);
                            output_ptr = output_ptr.offset(1);
                            if REMAINING < NUM_BITS {
                                out_register = right_shift_32(in_register, REMAINING as i32);
                            }
                        }
                    }

                    let in_register: DataType = delta_computer.transform(load_unaligned(input_ptr));
                    input_ptr = input_ptr.add(1);
                    let shifted = left_shift_32(in_register, 32 - NUM_BITS as i32);
                    out_register = op_or(out_register, shifted);
                    store_unaligned(output_ptr, out_register);
                    output_ptr = output_ptr.add(1);
                }

                NUM_BYTES_PER_BLOCK
            }

            #[$cpufeature]
            #[unroll_for_loops]
            pub(crate) unsafe fn unpack<Output: Sink>(
                compressed: &[u8],
                mut output: Output,
            ) -> usize {
                assert!(
                    compressed.len() >= NUM_BYTES_PER_BLOCK,
                    "Compressed array seems too small. ({} < {}) ",
                    compressed.len(),
                    NUM_BYTES_PER_BLOCK
                );

                let mut input_ptr = compressed.as_ptr() as *const DataType;

                const MASK_SCALAR: u32 = ((1u64 << NUM_BITS) - 1u64) as u32;
                let mask: DataType = set1(MASK_SCALAR as i32);

                for _ in 0..REPEAT {
                    let mut in_register: DataType = load_unaligned(input_ptr);
                    let out_register = op_and(in_register, mask);
                    output.process(out_register);

                    for i in 1..$cycle {
                        const INNER_CURSOR: usize = (i * NUM_BITS) % 32;
                        const INNER_CAPACITY: usize = 32 - INNER_CURSOR;

                        // LLVM will not emit the shift operand if
                        // `INNER_CURSOR` is 0.
                        let shifted_in_register = right_shift_32(in_register, INNER_CURSOR as i32);
                        let mut out_register: DataType = op_and(shifted_in_register, mask);

                        // We consumed our current quadruplets entirely.
                        // We therefore read another one.
                        if INNER_CAPACITY <= NUM_BITS && i != $cycle - 1 {
                            input_ptr = input_ptr.add(1);
                            in_register = load_unaligned(input_ptr);

                            // This quadruplets is actually cutting one of
                            // our `DataType`. We need to read the next one.
                            if INNER_CAPACITY < NUM_BITS {
                                let shifted = left_shift_32(in_register, INNER_CAPACITY as i32);
                                let masked = op_and(shifted, mask);
                                out_register = op_or(out_register, masked);
                            }
                        }

                        output.process(out_register);
                    }
                    input_ptr = input_ptr.add(1);
                }
                NUM_BYTES_PER_BLOCK
            }
        }
    };
}

macro_rules! pack_unpack_with_bits_32 {
    ($cpufeature:meta) => {
        mod pack_unpack_with_bits_32 {

            use super::BLOCK_LEN;
            use super::{load_unaligned, store_unaligned, DataType};
            use super::{Sink, Transformer};
            use unroll::unroll_for_loops;

            const NUM_BITS: usize = 32;
            const NUM_BYTES_PER_BLOCK: usize = NUM_BITS * BLOCK_LEN / 8;

            #[$cpufeature]
            #[unroll_for_loops]
            pub(crate) unsafe fn pack<TDeltaComputer: Transformer>(
                input_arr: &[u32],
                output_arr: &mut [u8],
                mut delta_computer: TDeltaComputer,
            ) -> usize {
                assert_eq!(
                    input_arr.len(),
                    BLOCK_LEN,
                    "Input block too small {}, (expected {})",
                    input_arr.len(),
                    BLOCK_LEN
                );
                assert!(
                    output_arr.len() >= NUM_BYTES_PER_BLOCK,
                    "Output array too small (numbits {}). {} <= {}",
                    NUM_BITS,
                    output_arr.len(),
                    NUM_BYTES_PER_BLOCK
                );

                let input_ptr: *const DataType = input_arr.as_ptr() as *const DataType;
                let output_ptr = output_arr.as_mut_ptr() as *mut DataType;
                for i in 0..32 {
                    let input_offset_ptr = input_ptr.offset(i as isize);
                    let output_offset_ptr = output_ptr.offset(i as isize);
                    let input_register = load_unaligned(input_offset_ptr);
                    let output_register = delta_computer.transform(input_register);
                    store_unaligned(output_offset_ptr, output_register);
                }
                NUM_BYTES_PER_BLOCK
            }

            #[$cpufeature]
            pub(crate) unsafe fn unpack<Output: Sink>(
                compressed: &[u8],
                mut output: Output,
            ) -> usize {
                assert!(
                    compressed.len() >= NUM_BYTES_PER_BLOCK,
                    "Compressed array seems too small. ({} < {}) ",
                    compressed.len(),
                    NUM_BYTES_PER_BLOCK
                );
                let input_ptr = compressed.as_ptr() as *const DataType;
                for i in 0..32 {
                    let input_offset_ptr = input_ptr.offset(i as isize);
                    let in_register: DataType = load_unaligned(input_offset_ptr);
                    output.process(in_register);
                }
                NUM_BYTES_PER_BLOCK
            }
        }
    };
}

macro_rules! declare_bitpacker {
    ($cpufeature:meta) => {
        use super::super::UnsafeBitPacker;
        use crate::most_significant_bit;
        use unroll::unroll_for_loops;

        pack_unpack_with_bits!(pack_unpack_with_bits_1, 1, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_2, 2, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_3, 3, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_4, 4, 8, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_5, 5, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_6, 6, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_7, 7, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_8, 8, 4, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_9, 9, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_10, 10, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_11, 11, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_12, 12, 8, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_13, 13, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_14, 14, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_15, 15, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_16, 16, 2, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_17, 17, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_18, 18, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_19, 19, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_20, 20, 8, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_21, 21, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_22, 22, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_23, 23, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_24, 24, 4, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_25, 25, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_26, 26, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_27, 27, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_28, 28, 8, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_29, 29, 32, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_30, 30, 16, $cpufeature);
        pack_unpack_with_bits!(pack_unpack_with_bits_31, 31, 32, $cpufeature);
        pack_unpack_with_bits_32!($cpufeature);

        unsafe fn compress_generic<DeltaComputer: Transformer>(
            decompressed: &[u32],
            compressed: &mut [u8],
            num_bits: u8,
            delta_computer: DeltaComputer,
        ) -> usize {
            match num_bits {
                0 => 0,
                1 => pack_unpack_with_bits_1::pack(decompressed, compressed, delta_computer),
                2 => pack_unpack_with_bits_2::pack(decompressed, compressed, delta_computer),
                3 => pack_unpack_with_bits_3::pack(decompressed, compressed, delta_computer),
                4 => pack_unpack_with_bits_4::pack(decompressed, compressed, delta_computer),
                5 => pack_unpack_with_bits_5::pack(decompressed, compressed, delta_computer),
                6 => pack_unpack_with_bits_6::pack(decompressed, compressed, delta_computer),
                7 => pack_unpack_with_bits_7::pack(decompressed, compressed, delta_computer),
                8 => pack_unpack_with_bits_8::pack(decompressed, compressed, delta_computer),
                9 => pack_unpack_with_bits_9::pack(decompressed, compressed, delta_computer),
                10 => pack_unpack_with_bits_10::pack(decompressed, compressed, delta_computer),
                11 => pack_unpack_with_bits_11::pack(decompressed, compressed, delta_computer),
                12 => pack_unpack_with_bits_12::pack(decompressed, compressed, delta_computer),
                13 => pack_unpack_with_bits_13::pack(decompressed, compressed, delta_computer),
                14 => pack_unpack_with_bits_14::pack(decompressed, compressed, delta_computer),
                15 => pack_unpack_with_bits_15::pack(decompressed, compressed, delta_computer),
                16 => pack_unpack_with_bits_16::pack(decompressed, compressed, delta_computer),
                17 => pack_unpack_with_bits_17::pack(decompressed, compressed, delta_computer),
                18 => pack_unpack_with_bits_18::pack(decompressed, compressed, delta_computer),
                19 => pack_unpack_with_bits_19::pack(decompressed, compressed, delta_computer),
                20 => pack_unpack_with_bits_20::pack(decompressed, compressed, delta_computer),
                21 => pack_unpack_with_bits_21::pack(decompressed, compressed, delta_computer),
                22 => pack_unpack_with_bits_22::pack(decompressed, compressed, delta_computer),
                23 => pack_unpack_with_bits_23::pack(decompressed, compressed, delta_computer),
                24 => pack_unpack_with_bits_24::pack(decompressed, compressed, delta_computer),
                25 => pack_unpack_with_bits_25::pack(decompressed, compressed, delta_computer),
                26 => pack_unpack_with_bits_26::pack(decompressed, compressed, delta_computer),
                27 => pack_unpack_with_bits_27::pack(decompressed, compressed, delta_computer),
                28 => pack_unpack_with_bits_28::pack(decompressed, compressed, delta_computer),
                29 => pack_unpack_with_bits_29::pack(decompressed, compressed, delta_computer),
                30 => pack_unpack_with_bits_30::pack(decompressed, compressed, delta_computer),
                31 => pack_unpack_with_bits_31::pack(decompressed, compressed, delta_computer),
                32 => pack_unpack_with_bits_32::pack(decompressed, compressed, delta_computer),
                _ => {
                    panic!("Num bits must be <= 32. Was {}.", num_bits);
                }
            }
        }

        pub trait Transformer {
            unsafe fn transform(&mut self, data: DataType) -> DataType;
        }

        struct NoDelta;

        impl Transformer for NoDelta {
            #[inline]
            unsafe fn transform(&mut self, current: DataType) -> DataType {
                current
            }
        }

        struct DeltaComputer {
            pub previous: DataType,
        }

        impl Transformer for DeltaComputer {
            #[inline]
            unsafe fn transform(&mut self, current: DataType) -> DataType {
                let result = compute_delta(current, self.previous);
                self.previous = current;
                result
            }
        }

        pub trait Sink {
            unsafe fn process(&mut self, data_type: DataType);
        }

        struct Store {
            output_ptr: *mut DataType,
        }

        impl Store {
            fn new(output_ptr: *mut DataType) -> Store {
                Store { output_ptr }
            }
        }

        struct DeltaIntegrate {
            current: DataType,
            output_ptr: *mut DataType,
        }

        impl DeltaIntegrate {
            unsafe fn new(initial: u32, output_ptr: *mut DataType) -> DeltaIntegrate {
                DeltaIntegrate {
                    current: set1(initial as i32),
                    output_ptr,
                }
            }
        }

        impl Sink for DeltaIntegrate {
            #[inline]
            unsafe fn process(&mut self, delta: DataType) {
                self.current = integrate_delta(self.current, delta);
                store_unaligned(self.output_ptr, self.current);
                self.output_ptr = self.output_ptr.add(1);
            }
        }

        impl Sink for Store {
            #[inline]
            unsafe fn process(&mut self, out_register: DataType) {
                store_unaligned(self.output_ptr, out_register);
                self.output_ptr = self.output_ptr.add(1);
            }
        }

        #[inline]
        unsafe fn decompress_to<Output: Sink>(
            compressed: &[u8],
            mut sink: Output,
            num_bits: u8,
        ) -> usize {
            match num_bits {
                0 => {
                    let zero = set1(0i32);
                    for _ in 0..32 {
                        sink.process(zero);
                    }
                    0
                }
                1 => pack_unpack_with_bits_1::unpack(compressed, sink),
                2 => pack_unpack_with_bits_2::unpack(compressed, sink),
                3 => pack_unpack_with_bits_3::unpack(compressed, sink),
                4 => pack_unpack_with_bits_4::unpack(compressed, sink),
                5 => pack_unpack_with_bits_5::unpack(compressed, sink),
                6 => pack_unpack_with_bits_6::unpack(compressed, sink),
                7 => pack_unpack_with_bits_7::unpack(compressed, sink),
                8 => pack_unpack_with_bits_8::unpack(compressed, sink),
                9 => pack_unpack_with_bits_9::unpack(compressed, sink),
                10 => pack_unpack_with_bits_10::unpack(compressed, sink),
                11 => pack_unpack_with_bits_11::unpack(compressed, sink),
                12 => pack_unpack_with_bits_12::unpack(compressed, sink),
                13 => pack_unpack_with_bits_13::unpack(compressed, sink),
                14 => pack_unpack_with_bits_14::unpack(compressed, sink),
                15 => pack_unpack_with_bits_15::unpack(compressed, sink),
                16 => pack_unpack_with_bits_16::unpack(compressed, sink),
                17 => pack_unpack_with_bits_17::unpack(compressed, sink),
                18 => pack_unpack_with_bits_18::unpack(compressed, sink),
                19 => pack_unpack_with_bits_19::unpack(compressed, sink),
                20 => pack_unpack_with_bits_20::unpack(compressed, sink),
                21 => pack_unpack_with_bits_21::unpack(compressed, sink),
                22 => pack_unpack_with_bits_22::unpack(compressed, sink),
                23 => pack_unpack_with_bits_23::unpack(compressed, sink),
                24 => pack_unpack_with_bits_24::unpack(compressed, sink),
                25 => pack_unpack_with_bits_25::unpack(compressed, sink),
                26 => pack_unpack_with_bits_26::unpack(compressed, sink),
                27 => pack_unpack_with_bits_27::unpack(compressed, sink),
                28 => pack_unpack_with_bits_28::unpack(compressed, sink),
                29 => pack_unpack_with_bits_29::unpack(compressed, sink),
                30 => pack_unpack_with_bits_30::unpack(compressed, sink),
                31 => pack_unpack_with_bits_31::unpack(compressed, sink),
                32 => pack_unpack_with_bits_32::unpack(compressed, sink),
                _ => {
                    panic!("Num bits must be <= 32. Was {}.", num_bits);
                }
            }
        }

        pub struct UnsafeBitPackerImpl;

        impl UnsafeBitPacker for UnsafeBitPackerImpl {
            const BLOCK_LEN: usize = BLOCK_LEN;

            #[$cpufeature]
            unsafe fn compress(decompressed: &[u32], compressed: &mut [u8], num_bits: u8) -> usize {
                compress_generic(decompressed, compressed, num_bits, NoDelta)
            }

            #[$cpufeature]
            unsafe fn compress_sorted(
                initial: u32,
                decompressed: &[u32],
                compressed: &mut [u8],
                num_bits: u8,
            ) -> usize {
                let delta_computer = DeltaComputer {
                    previous: set1(initial as i32),
                };
                compress_generic(decompressed, compressed, num_bits, delta_computer)
            }

            #[$cpufeature]
            unsafe fn decompress(
                compressed: &[u8],
                decompressed: &mut [u32],
                num_bits: u8,
            ) -> usize {
                assert!(
                    decompressed.len() >= BLOCK_LEN,
                    "The output array is not large enough : ({} >= {})",
                    decompressed.len(),
                    BLOCK_LEN
                );
                let output_ptr = decompressed.as_mut_ptr() as *mut DataType;
                let output = Store::new(output_ptr);
                decompress_to(compressed, output, num_bits)
            }

            #[$cpufeature]
            unsafe fn decompress_sorted(
                initial: u32,
                compressed: &[u8],
                decompressed: &mut [u32],
                num_bits: u8,
            ) -> usize {
                assert!(
                    decompressed.len() >= BLOCK_LEN,
                    "The output array is not large enough : ({} >= {})",
                    decompressed.len(),
                    BLOCK_LEN
                );
                let output_ptr = decompressed.as_mut_ptr() as *mut DataType;
                let output = DeltaIntegrate::new(initial, output_ptr);
                decompress_to(compressed, output, num_bits)
            }

            #[$cpufeature]
            #[unroll_for_loops]
            unsafe fn num_bits(decompressed: &[u32]) -> u8 {
                assert_eq!(
                    decompressed.len(),
                    BLOCK_LEN,
                    "`decompressed`'s len is not `BLOCK_LEN={}`",
                    BLOCK_LEN
                );
                let data: *const DataType = decompressed.as_ptr() as *const DataType;
                let mut accumulator = load_unaligned(data);
                for iter in 0..31 {
                    let i = iter + 1;
                    let newvec = load_unaligned(data.add(i));
                    accumulator = op_or(accumulator, newvec);
                }
                most_significant_bit(or_collapse_to_u32(accumulator))
            }

            #[$cpufeature]
            #[unroll_for_loops]
            unsafe fn num_bits_sorted(initial: u32, decompressed: &[u32]) -> u8 {
                assert_eq!(
                    decompressed.len(),
                    BLOCK_LEN,
                    "`decompressed`'s len is not `BLOCK_LEN={}`",
                    BLOCK_LEN
                );
                let initial_vec = set1(initial as i32);
                let data: *const DataType = decompressed.as_ptr() as *const DataType;

                let first = load_unaligned(data);
                let mut accumulator = compute_delta(load_unaligned(data), initial_vec);
                let mut previous = first;

                for iter in 0..30 {
                    let i = iter + 1;
                    let current = load_unaligned(data.add(i));
                    let delta = compute_delta(current, previous);
                    accumulator = op_or(accumulator, delta);
                    previous = current;
                }
                let current = load_unaligned(data.add(31));
                let delta = compute_delta(current, previous);
                accumulator = op_or(accumulator, delta);
                most_significant_bit(or_collapse_to_u32(accumulator))
            }
        }

        #[cfg(test)]
        mod tests {
            use super::UnsafeBitPackerImpl;
            use crate::tests::test_suite_compress_decompress;
            use crate::Available;
            use crate::UnsafeBitPacker;

            #[test]
            fn test_num_bits() {
                if UnsafeBitPackerImpl::available() {
                    for num_bits in 0..32 {
                        for pos in 0..32 {
                            let mut vals = [0u32; UnsafeBitPackerImpl::BLOCK_LEN];
                            if num_bits > 0 {
                                vals[pos] = 1 << (num_bits - 1);
                            }
                            assert_eq!(
                                unsafe { UnsafeBitPackerImpl::num_bits(&vals[..]) },
                                num_bits
                            );
                        }
                    }
                }
            }

            #[test]
            fn test_bitpacker() {
                if UnsafeBitPackerImpl::available() {
                    test_suite_compress_decompress::<UnsafeBitPackerImpl>(false);
                }
            }

            #[test]
            fn test_bitpacker_delta() {
                if UnsafeBitPackerImpl::available() {
                    test_suite_compress_decompress::<UnsafeBitPackerImpl>(true);
                }
            }
        }
    };
}
