use std::{error::Error, io::Read};

use crate::table_b;

// Use a buffer 8 times the size of the largest set of bits we need to read.
// Notice we're going from bits to bytes here.
const BUF_SIZE: usize = table_b::MAX_BIT_WIDTH;
const BYTE_ARRAY_SIZE: usize = if table_b::MAX_BIT_WIDTH % 8 == 0 {
    table_b::MAX_BIT_WIDTH / 8
} else {
    table_b::MAX_BIT_WIDTH / 8 + 1
};

pub(crate) struct BitBuffer<'b> {
    // The source
    reader: &'b mut dyn Read,

    // Track where we are in the source.
    max_bytes_to_read: usize,
    bytes_read: usize,

    // The buffer
    buffer: [u8; BUF_SIZE],
    buffer_len: usize,

    // Track where we are in the buffer
    byte_position: usize,
    bit_position: usize,
}

impl<'b> BitBuffer<'b> {
    pub fn new(reader: &'b mut dyn Read, max_bytes_to_read: usize) -> Self {
        BitBuffer {
            reader,
            max_bytes_to_read,
            bytes_read: 0,
            buffer: [0; BUF_SIZE],
            byte_position: 0,
            bit_position: 0,
            buffer_len: 0,
        }
    }

    pub fn bytes_read(&self) -> usize {
        self.bytes_read
    }

    fn num_bytes_to_hold_bits(n: usize) -> usize {
        if n % 8 == 0 {
            n / 8
        } else {
            n / 8 + 1
        }
    }

    fn read_n_bits(&mut self, n: usize) -> Result<Option<[u8; BYTE_ARRAY_SIZE]>, Box<dyn Error>> {
        let mut mask = [255u8; BYTE_ARRAY_SIZE];
        let mut vals = [0u8; BYTE_ARRAY_SIZE];

        // Bookkeeping
        let most_sig_byte = BYTE_ARRAY_SIZE - BitBuffer::num_bytes_to_hold_bits(n);
        let bits_first_byte = if n % 8 == 0 { 8 } else { n % 8 };

        // Build the mask
        for i in 0..most_sig_byte {
            mask[i] = 0;
        }
        mask[most_sig_byte] >>= 8 - bits_first_byte;

        // Load the bytes
        vals[most_sig_byte] = mask[most_sig_byte] & self.read_u8(bits_first_byte)?;
        for i in (most_sig_byte + 1)..BYTE_ARRAY_SIZE {
            vals[i] = mask[i] & self.read_u8(8)?;
        }

        // Check for BUFR missing value (all bits are set to 1
        if vals == mask {
            Ok(None)
        } else {
            Ok(Some(vals))
        }
    }

    fn bits_remaining_in_buffer(&self) -> usize {
        (self.buffer_len - self.byte_position) * 8 - self.bit_position
    }

    fn refill_buffer(&mut self) -> Result<(), Box<dyn Error>> {
        if self.max_bytes_to_read - self.bytes_read >= BUF_SIZE {
            self.reader.read_exact(&mut self.buffer)?;
            self.buffer_len = BUF_SIZE;
            self.bytes_read += BUF_SIZE;
        } else {
            let num_bytes_remaining = self.max_bytes_to_read - self.bytes_read;
            let mut buf = &mut self.buffer[0..num_bytes_remaining];
            self.reader.read_exact(&mut buf)?;
            self.buffer_len = num_bytes_remaining;
            self.bytes_read += num_bytes_remaining;
        }

        self.byte_position = 0;
        self.bit_position = 0;

        Ok(())
    }

    fn next_byte(&mut self) -> Result<u8, Box<dyn Error>> {
        if self.bits_remaining_in_buffer() == 0 {
            self.refill_buffer()?;
        }

        Ok(self.buffer[self.byte_position])
    }

    fn read_u8(&mut self, bits: usize) -> Result<u8, Box<dyn Error>> {
        debug_assert!(bits <= 8, "bits too large {} > 8", bits);
        debug_assert!(bits > 0, "requested zero bits");

        //dbg!(bits);

        let mut val: u8 = 0;

        let bits_left_in_byte = 8 - self.bit_position;
        if bits_left_in_byte < bits {
            //dbg!("Not all my bits are in this byte.");
            // Not all my bits are in this byte

            let byte = self.next_byte()?;

            // Need to get the rightmost bits
            let mask = 0b1111_1111 >> (8 - bits_left_in_byte);
            val |= byte & mask;
            // Need to left shift by how much?
            let num_bits_in_next_byte = bits - bits_left_in_byte;
            val <<= num_bits_in_next_byte;

            // Move to the next byte in the buffer
            self.bit_position = 0;
            self.byte_position += 1;

            // Get the next byte
            let mut byte = self.next_byte()?;

            // Get the leftmost how many bits?
            byte >>= 8 - num_bits_in_next_byte;
            val |= byte;

            // Advance the bit buffer
            self.bit_position += num_bits_in_next_byte;
        } else {
            //dbg!("All my bits are in this byte.", self.bit_position);
            // All my bits are here
            let mut byte = self.next_byte()?;

            // Example - self.bit_position = 1
            //           bits = 5
            //           mask =    0b0111_1100
            //
            //           num ->    0b0001_1111
            //           offset -> 0b0111_1100
            //           byte ->   0b0001_1111

            // Build the mask
            // Get a mask the right size first
            let mut mask = 0b1111_1111 >> (8 - bits);
            // Now give it the correct offset
            let offset = 8 - self.bit_position - bits;
            mask <<= offset;
            byte &= mask;
            // undo offset
            byte >>= offset;
            val = byte;

            // Advance the bit buffer
            self.bit_position += bits;
            if self.bit_position == 8 {
                self.bit_position = 0;
                self.byte_position += 1;
            }
            debug_assert!(
                self.bit_position < 8,
                "self.bit_postion = {}",
                self.bit_position
            );
        }

        debug_assert!(
            (val as u16) < (1u16 << bits),
            "value too big: {} >= {}",
            val,
            1u16 << bits
        );
        Ok(val)
    }

    pub fn read_text(&mut self, bits: usize) -> Result<String, Box<dyn Error>> {
        debug_assert_eq!(bits % 8, 0, "funky string size");

        let num_chars = bits / 8;
        //dbg!(num_chars, bits);
        let mut buf: Vec<u8> = Vec::with_capacity(num_chars);
        for _ in 0..num_chars {
            let c = self.read_u8(8)?;
            //dbg!(c);
            buf.push(c);
        }

        Ok(String::from_utf8(buf)?)
    }

    fn read_u64(&mut self, bits: usize) -> Result<Option<u64>, Box<dyn Error>> {
        debug_assert!(bits <= (8 * 8), "too many bits for u64: {}", bits);
        debug_assert!(bits > 0, "requested zero bits");

        let vals_buf = self.read_n_bits(bits)?;
        if let Some(vals_buf) = vals_buf {
            let mut small_buf: [u8; 8] = [0; 8];
            small_buf.clone_from_slice(&vals_buf[(BYTE_ARRAY_SIZE - 8)..]);
            let val = u64::from_be_bytes(small_buf);
            debug_assert!(
                val < (1u64 << bits),
                "val too large: {} >= {}",
                val,
                1u64 << bits
            );
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    pub fn read_usize(&mut self, bits: usize) -> Result<Option<usize>, Box<dyn Error>> {
        let val = self.read_u64(bits)?;
        match val {
            Some(val) => Ok(Some(usize::try_from(val)?)),
            None => Ok(None),
        }
    }

    pub fn read_i64(
        &mut self,
        bits: usize,
        reference_val: i64,
    ) -> Result<Option<i64>, Box<dyn Error>> {
        let val = self.read_u64(bits)?;

        match val {
            Some(val) => Ok(Some(i64::try_from(val)?)),
            None => Ok(None),
        }
    }

    pub fn read_f64(
        &mut self,
        bits: usize,
        reference_val: i64,
        scale: i32,
    ) -> Result<Option<f64>, Box<dyn Error>> {
        let mut val = self
            .read_i64(bits, reference_val)?
            .map(|v| v as f64)
            .map(|v| {
                if scale != 0 {
                    v / f64::powi(10.0, scale)
                } else {
                    v
                }
            });

        Ok(val)
    }
}
