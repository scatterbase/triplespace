// The browser's solver against SHA-256's own test values and against the test vectors of
// scatter-challenge, the crate the server checks solutions with (ADR 0087 §3, A1).
import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PrefixHasher, sha256Hex } from './sha256.js';
import { leadingZeroBits, solve } from './solver.js';
import { initChallengeForm } from './form.js';

const here = dirname( fileURLToPath( import.meta.url ) );
const file = join( here, '../../../crates/scatter-challenge/vectors.json' );
const vectors = JSON.parse( readFileSync( file, 'utf8' ) ).vectors;
const enc = ( s ) => new TextEncoder().encode( s );
// eslint-disable-next-line no-bitwise
const hex = ( h ) => Array.from( h, ( w ) => ( w >>> 0 ).toString( 16 ).padStart( 8, '0' ) ).join( '' );

describe( 'sha256', () => {
	it( 'gives FIPS 180-4\'s values', () => {
		expect( sha256Hex( enc( '' ) ) )
			.toBe( 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855' );
		expect( sha256Hex( enc( 'abc' ) ) )
			.toBe( 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' );
		expect( sha256Hex( enc( 'abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq' ) ) )
			.toBe( '248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1' );
		expect( sha256Hex( enc( 'a'.repeat( 1000 ) ) ) )
			.toBe( '41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3' );
	} );

	it( 'hashes a prefix and a suffix as one message, at every length', () => {
		for ( let p = 0; p < 140; p++ ) {
			const prefix = 'x'.repeat( p ) + ':';
			const h = new PrefixHasher( enc( prefix ) );
			for ( const suffix of [ '0', '7', '123456', '18446744073709551615' ] ) {
				const whole = sha256Hex( enc( prefix + suffix ) );
				expect( hex( h.digest( enc( suffix ) ) ) ).toBe( whole );
			}
		}
	} );
} );

describe( 'solver', () => {
	it( 'finds the smallest solution the crate finds', () => {
		expect( vectors.length ).toBeGreaterThanOrEqual( 4 );
		for ( const v of vectors ) {
			expect( solve( v.challenge, v.bits ) ).toBe( v.solution );
			expect( sha256Hex( enc( `${ v.challenge }:${ v.solution }` ) ) ).toBe( v.hash );
		}
	} );

	it( 'counts zero bits', () => {
		const words = ( first, second ) => new Uint32Array( [ first, second, 0, 0, 0, 0, 0, 0 ] );
		expect( leadingZeroBits( words( 0, 0x00010000 ) ) ).toBe( 47 );
		expect( leadingZeroBits( words( 0x80000000, 0 ) ) ).toBe( 0 );
		expect( leadingZeroBits( new Uint32Array( 8 ) ) ).toBe( 256 );
	} );

	it( 'reports progress and can stop', () => {
		const seen = [];
		expect( solve( 'x', 40, ( n ) => {
			seen.push( n );
			return seen.length < 2;
		} ) ).toBe( null );
		expect( seen ).toEqual( [ 65536, 131072 ] );
	} );
} );

describe( 'a form with a challenge', () => {
	const v = vectors[ 0 ];
	const markup = ( solution ) => `<form data-ts-challenge="${ v.bits }"><input name="tschallengeid" data-ts-challenge-bits="${ v.bits }" value="${ v.challenge }"><input name="tschallengesolution" value="${ solution }"><p data-ts-challenge-status data-working="working" data-waiting="waiting" data-done="done" data-failed="failed"></p><button type="submit">Go</button></form>`;

	it( 'solves while the person types, and holds a submission until then', async () => {
		document.body.innerHTML = markup( '' );
		const form = document.querySelector( 'form' );
		let worker;
		const fake = {
			postMessage: vi.fn( function () {
				worker = this;
			} ),
			terminate: vi.fn()
		};
		const sent = vi.fn();
		form.requestSubmit = sent;
		initChallengeForm( form, () => fake );
		const status = form.querySelector( '[data-ts-challenge-status]' );
		expect( status.textContent ).toBe( 'working' );
		expect( fake.postMessage ).toHaveBeenCalledWith( { challenge: v.challenge, bits: v.bits } );
		const e = new Event( 'submit', { cancelable: true } );
		form.dispatchEvent( e );
		expect( e.defaultPrevented ).toBe( true );
		expect( status.textContent ).toBe( 'waiting' );
		worker.onmessage( { data: { solution: v.solution } } );
		expect( form.querySelector( '[name="tschallengesolution"]' ).value ).toBe( String( v.solution ) );
		expect( status.textContent ).toBe( 'done' );
		expect( sent ).toHaveBeenCalledTimes( 1 );
		const again = new Event( 'submit', { cancelable: true } );
		form.dispatchEvent( again );
		expect( again.defaultPrevented ).toBe( false );
	} );

	it( 'solves on the page where there is no worker, and keeps a solution it has', async () => {
		document.body.innerHTML = markup( '' );
		const form = document.querySelector( 'form' );
		initChallengeForm( form, () => null );
		await new Promise( ( r ) => {
			setTimeout( r, 50 );
		} );
		expect( form.querySelector( '[name="tschallengesolution"]' ).value ).toBe( String( v.solution ) );
		document.body.innerHTML = markup( '42' );
		const kept = document.querySelector( 'form' );
		const make = vi.fn();
		initChallengeForm( kept, make );
		expect( make ).not.toHaveBeenCalled();
	} );
} );
