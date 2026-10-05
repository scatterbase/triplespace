/**
 * The labels and data types of the entities an editor shows, from `wbgetentities`, in
 * calls of at most 50.
 */

import type { Api } from './api';

/** Labels by ID, in the interface language or its fallbacks, and properties' data types. */
export interface Facts {
	labels: Record<string, string>;
	datatypes: Record<string, string>;
}

/**
 * The labels and data types of some entities.
 *
 * @param api The API client
 * @param lang The interface language
 * @param ids The IDs
 * @return Their labels and data types
 */
export async function describe( api: Api, lang: string, ids: string[] ): Promise<Facts> {
	const out: Facts = { labels: {}, datatypes: {} };
	const unique = [ ...new Set( ids.filter( ( id ) => id ) ) ];
	const chain = [ lang, 'mul', 'en' ];
	for ( let i = 0; i < unique.length; i += 50 ) {
		const body = await api.get( {
			action: 'wbgetentities',
			ids: unique.slice( i, i + 50 ).join( '|' ),
			props: 'labels|datatype',
		} );
		const entities = ( body.entities ?? {} ) as Record<string, {
			labels?: Record<string, { value: string }>;
			datatype?: string;
		}>;
		for ( const [ id, e ] of Object.entries( entities ) ) {
			const labels = e.labels ?? {};
			const l = chain.map( ( c ) => labels[ c ] ).find( Boolean ) ??
				Object.values( labels )[ 0 ];
			if ( l ) {
				out.labels[ id ] = l.value;
			}
			if ( e.datatype ) {
				out.datatypes[ id ] = e.datatype;
			}
		}
	}
	return out;
}
