/**
 * The mapping of an uploaded file's columns onto the table's (ADR 0106 §7; 21 §6.8): what
 * each source column becomes, the suggestions that are evident, and the table that
 * results.
 */

import { column, headerOf, isProperty } from './text';
import type { Column } from './text';

/** What a source column is mapped to. */
export type Target = 'ignore' | 'id' | 'label' | 'description' | 'aliases' | 'property' | 'new' | 'reads' | 'reference';

/** One source column's mapping. */
export interface Mapping {
	target: Target;
	/** A term's language. */
	language: string;
	/** An existing property's ID: the statements', or a reference value's. */
	property: string;
	/** Its label, where known. */
	propertyLabel: string;
	/** A new property's label and data type. */
	label: string;
	datatype: string;
	/** For a statement's own reference: the property whose statements carry it. */
	of: string;
}

/**
 * A mapping with nothing chosen.
 *
 * @param lang The wiki's language
 * @return The mapping
 */
export function emptyMapping( lang: string ): Mapping {
	return { target: 'ignore', language: lang, property: '', propertyLabel: '', label: '', datatype: '', of: '' };
}

/**
 * What a header suggests by itself: one already in the text's conventions, a property's
 * ID, or a name such as "title" or "description". A header that is a property's label is
 * suggested by the caller, which can search.
 *
 * @param header The source column's header
 * @param lang The wiki's language
 * @return The mapping, `ignore` where nothing is evident
 */
export function suggest( header: string, lang: string ): Mapping {
	const m = emptyMapping( lang );
	const c: Column = column( header, lang );
	switch ( c.kind ) {
		case 'id':
			return { ...m, target: 'id' };
		case 'label':
		case 'description':
		case 'aliases':
			return { ...m, target: c.kind, language: c.language ?? lang };
		case 'property':
			return { ...m, target: 'property', property: c.property ?? '' };
		case 'new':
			return { ...m, target: 'new', label: c.label ?? '', datatype: c.datatype ?? '' };
		case 'reference':
			return { ...m, target: 'reference', of: c.of ?? '', property: c.property ?? '' };
		default:
	}
	const name = header.trim().toLowerCase();
	if ( [ 'name', 'title', 'label' ].includes( name ) ) {
		return { ...m, target: 'label' };
	}
	if ( [ 'desc', 'description', 'summary' ].includes( name ) ) {
		return { ...m, target: 'description' };
	}
	if ( [ 'alias', 'aliases', 'also known as', 'synonyms' ].includes( name ) ) {
		return { ...m, target: 'aliases' };
	}
	if ( [ 'qid', 'item', 'item id', 'wikidata id' ].includes( name ) ) {
		return { ...m, target: 'id' };
	}
	return m;
}

/**
 * Whether a mapping is complete enough to apply.
 *
 * @param m The mapping
 * @return Whether it names what its target needs
 */
export function complete( m: Mapping ): boolean {
	switch ( m.target ) {
		case 'property':
			return isProperty( m.property );
		case 'new':
			return m.label.trim() !== '';
		case 'reads':
			return isProperty( m.property );
		case 'reference':
			return isProperty( m.property ) && isProperty( m.of );
		default:
			return true;
	}
}

/**
 * The table a mapped file becomes: its headers in the text's conventions, the ignored
 * columns left out, and the lines the reference on every statement gains for the columns
 * it reads.
 *
 * @param headers The source headers
 * @param rows The source rows
 * @param mappings One mapping per source column
 * @param lang The wiki's language
 * @return The table's rows, the header first, and the reference lines to add
 */
export function apply(
	headers: string[],
	rows: string[][],
	mappings: Mapping[],
	lang: string,
): { rows: string[][]; reference: string[] } {
	const keep: number[] = [];
	const out: string[] = [];
	const reference: string[] = [];
	const taken = new Set<string>();
	mappings.forEach( ( m, i ) => {
		let h: string;
		switch ( m.target ) {
			case 'ignore':
				return;
			case 'id':
				h = 'id';
				break;
			case 'label':
			case 'description':
			case 'aliases':
				h = headerOf( { kind: m.target, language: m.language.trim() || lang }, lang );
				break;
			case 'property':
				h = m.property.toUpperCase();
				break;
			case 'new':
				h = headerOf( { kind: 'new', label: m.label.trim(), datatype: m.datatype || undefined }, lang );
				break;
			case 'reference':
				h = `${ m.of.toUpperCase() }/ref:${ m.property.toUpperCase() }`;
				break;
			default: {
				// A plain column, read by the reference on every statement. Its header
				// must stay plain, so one that reads as something else is renamed.
				let name = headers[ i ].trim() || `column ${ i + 1 }`;
				if ( column( name, lang ).kind !== 'plain' ) {
					name = `${ name } (source)`;
				}
				while ( taken.has( name.toLowerCase() ) ) {
					name = `${ name } 2`;
				}
				h = name;
				reference.push( `${ m.property.toUpperCase() }:{${ name }}` );
			}
		}
		taken.add( h.toLowerCase() );
		keep.push( i );
		out.push( h );
	} );
	return {
		rows: [ out, ...rows.map( ( r ) => keep.map( ( i ) => r[ i ] ?? '' ) ) ],
		reference,
	};
}
