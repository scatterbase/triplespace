// The registration challenge's solver (ADR 0087 §3, A1): finds the smallest n such that
// SHA-256 of `{challenge}:{n}` begins with `bits` zero bits, the same answer
// scatter-challenge's native solver gives, as `challenge.test.js` checks against the
// crate's own test vectors.
/* eslint-disable no-bitwise -- counting bits of a digest */
import { PrefixHasher } from './sha256.js';

/**
 * The number of leading zero bits of a digest given as words.
 *
 * @param {Uint32Array} h
 * @return {number}
 */
export function leadingZeroBits( h ) {
	let n = 0;
	for ( let i = 0; i < 8; i++ ) {
		if ( h[ i ] === 0 ) {
			n += 32;
		} else {
			return n + Math.clz32( h[ i ] );
		}
	}
	return n;
}

/**
 * A number's decimal digits as bytes, written into a buffer from its end.
 *
 * @param {Uint8Array} buf 20 bytes
 * @param {number} n A safe integer
 * @return {Uint8Array} A view of the digits
 */
function digits( buf, n ) {
	let i = buf.length;
	do {
		buf[ --i ] = 48 + ( n % 10 );
		n = Math.floor( n / 10 );
	} while ( n > 0 );
	return buf.subarray( i );
}

/**
 * Solves a challenge, reporting progress every so many hashes. Stops early where
 * `progress` returns false.
 *
 * @param {string} challenge
 * @param {number} bits
 * @param {function(number):(boolean|void)} [progress] Called with the count tried.
 * @return {number|null} The solution, or null where stopped.
 */
export function solve( challenge, bits, progress ) {
	const hasher = new PrefixHasher( new TextEncoder().encode( challenge + ':' ) );
	const buf = new Uint8Array( 20 );
	for ( let n = 0; n < Number.MAX_SAFE_INTEGER; n++ ) {
		if ( leadingZeroBits( hasher.digest( digits( buf, n ) ) ) >= bits ) {
			return n;
		}
		if ( progress && ( n & 0xffff ) === 0xffff && progress( n + 1 ) === false ) {
			return null;
		}
	}
	return null;
}
