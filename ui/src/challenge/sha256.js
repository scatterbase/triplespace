// SHA-256 (FIPS 180-4) for the registration challenge (ADR 0087 §3, A1), written for one
// job: hashing `{challenge}:{n}` for n = 0, 1, 2, … as fast as plain JavaScript allows.
// The prefix's whole 64-byte blocks are compressed once (the midstate), so each candidate
// costs only its last block or two. The Web Crypto API answers each digest asynchronously,
// which makes a million of them slow, so the browser does not use it here.
/* eslint-disable no-bitwise -- a hash function is bitwise arithmetic */

const K = new Uint32Array( [
	0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
	0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
	0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
	0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
	0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
	0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
	0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
	0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
] );

const INITIAL = [
	0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
];

const W = new Uint32Array( 64 );

/**
 * Compresses one 64-byte block into the state.
 *
 * @param {Uint32Array} h The state, eight words, changed in place.
 * @param {Uint8Array} bytes
 * @param {number} at The block's offset in `bytes`.
 */
export function compress( h, bytes, at ) {
	for ( let i = 0; i < 16; i++ ) {
		const j = at + i * 4;
		W[ i ] = ( bytes[ j ] << 24 ) | ( bytes[ j + 1 ] << 16 ) |
			( bytes[ j + 2 ] << 8 ) | bytes[ j + 3 ];
	}
	for ( let i = 16; i < 64; i++ ) {
		const x = W[ i - 15 ];
		const y = W[ i - 2 ];
		const s0 = ( ( x >>> 7 ) | ( x << 25 ) ) ^ ( ( x >>> 18 ) | ( x << 14 ) ) ^ ( x >>> 3 );
		const s1 = ( ( y >>> 17 ) | ( y << 15 ) ) ^ ( ( y >>> 19 ) | ( y << 13 ) ) ^ ( y >>> 10 );
		W[ i ] = ( W[ i - 16 ] + s0 + W[ i - 7 ] + s1 ) | 0;
	}
	let a = h[ 0 ];
	let b = h[ 1 ];
	let c = h[ 2 ];
	let d = h[ 3 ];
	let e = h[ 4 ];
	let f = h[ 5 ];
	let g = h[ 6 ];
	let k = h[ 7 ];
	for ( let i = 0; i < 64; i++ ) {
		const s1 = ( ( e >>> 6 ) | ( e << 26 ) ) ^ ( ( e >>> 11 ) | ( e << 21 ) ) ^
			( ( e >>> 25 ) | ( e << 7 ) );
		const ch = ( e & f ) ^ ( ~e & g );
		const t1 = ( k + s1 + ch + K[ i ] + W[ i ] ) | 0;
		const s0 = ( ( a >>> 2 ) | ( a << 30 ) ) ^ ( ( a >>> 13 ) | ( a << 19 ) ) ^
			( ( a >>> 22 ) | ( a << 10 ) );
		const maj = ( a & b ) ^ ( a & c ) ^ ( b & c );
		const t2 = ( s0 + maj ) | 0;
		k = g;
		g = f;
		f = e;
		e = ( d + t1 ) | 0;
		d = c;
		c = b;
		b = a;
		a = ( t1 + t2 ) | 0;
	}
	h[ 0 ] = ( h[ 0 ] + a ) | 0;
	h[ 1 ] = ( h[ 1 ] + b ) | 0;
	h[ 2 ] = ( h[ 2 ] + c ) | 0;
	h[ 3 ] = ( h[ 3 ] + d ) | 0;
	h[ 4 ] = ( h[ 4 ] + e ) | 0;
	h[ 5 ] = ( h[ 5 ] + f ) | 0;
	h[ 6 ] = ( h[ 6 ] + g ) | 0;
	h[ 7 ] = ( h[ 7 ] + k ) | 0;
}

/**
 * Hashes a prefix followed by many suffixes: the prefix's whole blocks once, then each
 * suffix with the prefix's remainder.
 */
export class PrefixHasher {
	/**
	 * @param {Uint8Array} prefix
	 */
	constructor( prefix ) {
		const whole = prefix.length - ( prefix.length % 64 );
		this.mid = new Uint32Array( INITIAL );
		for ( let at = 0; at < whole; at += 64 ) {
			compress( this.mid, prefix, at );
		}
		this.prefixLength = prefix.length;
		this.rest = prefix.slice( whole );
		// Room for the rest, a suffix of up to 20 digits, the 0x80 and the length.
		this.buf = new Uint8Array( 128 );
		this.buf.set( this.rest );
		this.h = new Uint32Array( 8 );
	}

	/**
	 * The digest of the prefix and a suffix, as eight words (big-endian).
	 *
	 * @param {Uint8Array} suffix At most 20 bytes.
	 * @return {Uint32Array} Reused by the next call.
	 */
	digest( suffix ) {
		const buf = this.buf;
		const used = this.rest.length + suffix.length;
		buf.set( suffix, this.rest.length );
		buf[ used ] = 0x80;
		const blocks = used + 9 > 64 ? 2 : 1;
		buf.fill( 0, used + 1, blocks * 64 );
		const bits = ( this.prefixLength + suffix.length ) * 8;
		const end = blocks * 64;
		// Lengths under 2^32 bits: the high word stays zero.
		buf[ end - 4 ] = bits >>> 24;
		buf[ end - 3 ] = ( bits >>> 16 ) & 0xff;
		buf[ end - 2 ] = ( bits >>> 8 ) & 0xff;
		buf[ end - 1 ] = bits & 0xff;
		const h = this.h;
		h.set( this.mid );
		compress( h, buf, 0 );
		if ( blocks === 2 ) {
			compress( h, buf, 64 );
		}
		return h;
	}
}

/**
 * SHA-256 of bytes, as hex: for tests.
 *
 * @param {Uint8Array} bytes
 * @return {string}
 */
export function sha256Hex( bytes ) {
	const h = new Uint32Array( INITIAL );
	const total = bytes.length;
	const padded = new Uint8Array( ( ( total + 9 + 63 ) >> 6 ) << 6 );
	padded.set( bytes );
	padded[ total ] = 0x80;
	const bits = total * 8;
	const end = padded.length;
	padded[ end - 4 ] = bits >>> 24;
	padded[ end - 3 ] = ( bits >>> 16 ) & 0xff;
	padded[ end - 2 ] = ( bits >>> 8 ) & 0xff;
	padded[ end - 1 ] = bits & 0xff;
	padded[ end - 5 ] = Math.floor( bits / 0x100000000 ) & 0xff;
	for ( let at = 0; at < end; at += 64 ) {
		compress( h, padded, at );
	}
	return Array.from( h, ( w ) => ( w >>> 0 ).toString( 16 ).padStart( 8, '0' ) ).join( '' );
}
