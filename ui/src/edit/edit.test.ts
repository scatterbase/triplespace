import { describe, expect, it, vi } from 'vitest';
import { Api, ApiError } from './api';
import { msg } from './data';
import type { EditData } from './data';
import { refreshRegion, regionUrl } from './regions';
import { termsEdit, termsOf } from './terms';

const data: EditData = {
	id: 'Q6',
	type: 'item',
	title: 'Item:Q6',
	lang: 'en',
	dir: 'ltr',
	build: 'b1',
	messages: { 'ts-edit-failed': 'Not saved: $1', 'ts-edit-in-language': 'In $1' },
};

function json( body: unknown, status = 200 ): Response {
	return new Response( JSON.stringify( body ), { status, headers: { 'Content-Type': 'application/json' } } );
}

describe( 'messages', () => {
	it( 'replaces parameters and marks missing keys', () => {
		expect( msg( data, 'ts-edit-failed', 'busy' ) ).toBe( 'Not saved: busy' );
		expect( msg( data, 'ts-nope' ) ).toBe( '⧼ts-nope⧽' );
	} );
} );

describe( 'terms', () => {
	it( 'reads one language and writes it in one edit', () => {
		const t = termsOf( {
			labels: { en: { language: 'en', value: 'Six' }, de: { language: 'de', value: 'Sechs' } },
			aliases: { en: [ { language: 'en', value: '6' } ] },
		}, 'en' );
		expect( t ).toEqual( { label: 'Six', description: '', aliases: [ '6' ] } );
		expect( termsEdit( 'en', { label: '', description: 'd', aliases: [] } ) ).toEqual( {
			labels: { en: { language: 'en', value: '' } },
			descriptions: { en: { language: 'en', value: 'd' } },
			aliases: { en: [ { language: 'en', value: '' } ] },
		} );
	} );
} );

describe( 'api', () => {
	it( 'sends writes with the token, and retries once with a fresh one', async () => {
		const calls: [ string, RequestInit | undefined ][] = [];
		let tokens = 0;
		const fetcher = vi.fn( async ( url: string, init?: RequestInit ) => {
			calls.push( [ url, init ] );
			if ( url.includes( 'meta=tokens' ) ) {
				tokens++;
				return json( { query: { tokens: { csrftoken: `t${ tokens }+\\` } } } );
			}
			const body = new URLSearchParams( String( init?.body ) );
			if ( body.get( 'token' ) === 't1+\\' ) {
				return json( { error: { code: 'badtoken', info: 'stale' } } );
			}
			return json( { success: 1, entity: { lastrevid: 9 } } );
		} );
		const api = new Api( fetcher );
		const r = await api.write( { action: 'wbeditentity', id: 'Q6', baserevid: 3 } );
		expect( r.success ).toBe( 1 );
		expect( tokens ).toBe( 2 );
		const last = new URLSearchParams( String( calls[ calls.length - 1 ][ 1 ]?.body ) );
		expect( last.get( 'baserevid' ) ).toBe( '3' );
		expect( last.get( 'formatversion' ) ).toBe( '2' );
		expect( calls[ calls.length - 1 ][ 1 ]?.method ).toBe( 'POST' );
	} );

	it( 'raises the API\'s errors with their codes', async () => {
		const api = new Api( async () => json( { error: { code: 'editconflict', info: 'changed' } } ) );
		await expect( api.get( { action: 'query' } ) ).rejects.toMatchObject( { code: 'editconflict' } );
		await expect( api.get( { action: 'query' } ) ).rejects.toBeInstanceOf( ApiError );
	} );
} );

describe( 'regions', () => {
	it( 'asks for the region with the page\'s build and swaps it in', async () => {
		document.body.innerHTML = '<div data-region="terms"><p>old</p></div>';
		let sent: RequestInit | undefined;
		const fresh = await refreshRegion( document, data, 'terms', async ( url, init ) => {
			sent = init;
			expect( url ).toBe( regionUrl( data, 'terms' ) );
			return new Response( '<div class="ts-terms" data-region="terms"><p>new</p></div>' );
		} );
		expect( ( sent?.headers as Record<string, string> )[ 'X-Triplespace-UI-Build' ] ).toBe( 'b1' );
		expect( fresh?.textContent ).toBe( 'new' );
		expect( document.body.innerHTML ).toBe( '<div class="ts-terms" data-region="terms"><p>new</p></div>' );
		expect( regionUrl( data, 'statements/P31' ) ).toBe( '/w/index.php?title=Item%3AQ6&action=render&region=statements%2FP31&uselang=en' );
	} );

	it( 'reloads the page when the server is on another build', async () => {
		document.body.innerHTML = '<div data-region="terms">old</div>';
		const reload = vi.fn();
		const r = await refreshRegion( document, data, 'terms', async () => new Response( null, { status: 409 } ), reload );
		expect( r ).toBeNull();
		expect( reload ).toHaveBeenCalledOnce();
		expect( document.body.textContent ).toBe( 'old' );
	} );
} );
