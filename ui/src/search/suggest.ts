/**
 * The suggestions of the header's search box (ADR 0034 §4; chapter 19 §2.1): entities
 * by label or alias, a kind at a time, then pages by title, with a "Go to" first when
 * the text is an ID.
 */
import type { SearchResult } from '@wikimedia/codex';
import type { Api } from '../edit/api';
import { format } from '../i18n/format';

/** What the server writes beside the box (`ts-search-data`). */
export interface SearchData {
	/** The interface language, which the labels are searched in. */
	lang: string;
	/** The kinds of entity to suggest, in the order they are listed. */
	kinds: string[];
	/** Whether pages are suggested, by title. */
	pages: boolean;
	/** The `ts-typeahead-*` messages, unexpanded. */
	messages: Record<string, string>;
}

/** The most suggestions of one kind. */
export const PER_KIND = 4;

/** The most suggestions in all. */
export const MOST = 10;

/**
 * A message with `$1` replaced; `⧼key⧽` when it is missing, as MediaWiki shows one.
 *
 * @param data The search data
 * @param key The message key
 * @param param The parameter
 * @return The message
 */
export function message( data: SearchData, key: string, param = '' ): string {
	return format( data.messages, data.lang, key, [ param ] );
}

/**
 * The path of a page, as the site links one: spaces as underscores, and the characters
 * a title keeps readable left as they are.
 *
 * @param title The prefixed title
 * @return The path
 */
export function pagePath( title: string ): string {
	return '/wiki/' + encodeURIComponent( title.replace( / /g, '_' ) )
		.replace( /%3A/gi, ':' )
		.replace( /%2F/gi, '/' );
}

/**
 * Where the search page is asked for a text: with `fulltext`, its results; without, the
 * page the text names, if it names one (chapter 19 §2.1).
 *
 * @param query The text
 * @param results Whether to ask for the results whatever the text names
 * @return The URL
 */
export function searchUrl( query: string, results: boolean ): string {
	const q = new URLSearchParams( { title: 'Special:Search', search: query } );
	if ( results ) {
		q.set( 'fulltext', '1' );
	}
	return `/w/index.php?${ q.toString() }`;
}

/**
 * Whether a text is shaped as an entity ID, local or foreign: `Q42`, `P31`, `WDQ42`.
 *
 * @param text The text
 * @return Whether it is
 */
export function looksLikeId( text: string ): boolean {
	return /^[A-Za-z]{0,3}[PQpq][1-9]\d*$/.test( text );
}

interface EntityHit {
	id: string;
	title?: string;
	display?: { label?: { value: string }; description?: { value: string } };
}

async function entities(
	api: Api, data: SearchData, query: string, kind: string,
): Promise<SearchResult[]> {
	const body = await api.get( {
		action: 'wbsearchentities',
		search: query,
		language: data.lang,
		type: kind,
		limit: PER_KIND,
	} );
	return ( ( body.search ?? [] ) as EntityHit[] ).map( ( h ) => ( {
		value: h.id,
		label: h.display?.label?.value ?? h.id,
		description: h.display?.description?.value ?? null,
		supportingText: h.display?.label?.value ? `(${ h.id })` : undefined,
		url: pagePath( h.title ?? h.id ),
	} ) );
}

async function pages( api: Api, data: SearchData, query: string ): Promise<SearchResult[]> {
	const body = await api.get( {
		action: 'query',
		list: 'prefixsearch',
		pssearch: query,
		psnamespace: '*',
		pslimit: PER_KIND,
	} );
	const hits = ( ( body.query as { prefixsearch?: { title: string }[] } | undefined )
		?.prefixsearch ?? [] );
	return hits.map( ( h ) => ( {
		value: `page:${ h.title }`,
		label: h.title,
		description: message( data, 'ts-typeahead-page' ),
		url: pagePath( h.title ),
	} ) );
}

/**
 * The suggestions for a text: a "Go to" for an ID nothing else names, then each kind of
 * entity, then pages. A kind whose call fails is left out; the rest still show.
 *
 * @param api The API client
 * @param data The search data
 * @param text The text typed
 * @return The suggestions, at most `MOST`
 */
export async function suggest( api: Api, data: SearchData, text: string ): Promise<SearchResult[]> {
	const query = text.trim();
	if ( !query ) {
		return [];
	}
	const calls = data.kinds.map( ( k ) => entities( api, data, query, k ) );
	if ( data.pages ) {
		calls.push( pages( api, data, query ) );
	}
	const settled = await Promise.allSettled( calls );
	const found: SearchResult[] = [];
	const seen = new Set<string | number>();
	for ( const s of settled ) {
		if ( s.status !== 'fulfilled' ) {
			continue;
		}
		for ( const r of s.value ) {
			if ( !seen.has( r.value ) ) {
				seen.add( r.value );
				found.push( r );
			}
		}
	}
	const out = found.slice( 0, MOST );
	if ( looksLikeId( query ) ) {
		const id = query.toUpperCase();
		const at = out.findIndex( ( r ) => String( r.value ).toUpperCase() === id );
		if ( at > 0 ) {
			// The entity the ID names leads.
			out.unshift( ...out.splice( at, 1 ) );
		} else if ( at < 0 ) {
			out.unshift( {
				value: `goto:${ query }`,
				label: message( data, 'ts-typeahead-goto', query ),
				url: searchUrl( query, false ),
			} );
			out.splice( MOST );
		}
	}
	return out;
}
