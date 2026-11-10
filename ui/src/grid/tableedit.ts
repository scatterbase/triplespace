/**
 * What an edit of a Table page's grid becomes (ADR 0045 §6, §7; 15 §2.5, §2.6): for each
 * changed row one `wbeditentity` on the row's entity, and for rows added or removed one
 * edit of the page's definition.
 */

import { dataValueOf } from './plain';
import type { TableColumn, TableRow } from './data';
import { splitValues } from './text';

/** A statement as the API serves it. */
export interface Claim {
	id?: string;
	mainsnak: Record<string, unknown>;
	[ key: string ]: unknown;
}

/** Why a row's edit cannot be made, naming the column. */
export interface CellProblem {
	column: number;
	text: string;
}

/**
 * `wbeditentity`'s `data` for one row's changed cells.
 *
 * - A term changed is set; cleared, removed. Aliases are the language's whole list,
 *   `|`-separated.
 * - A value typed into an empty statement cell is a new statement, with the table's
 *   default reference where one is given.
 * - The one best value changed is the same statement with its main snak replaced, its
 *   qualifiers and references kept; cleared, the statement removed.
 *
 * @param columns The table's columns
 * @param before The row as it was read
 * @param after The cells as edited, one per column
 * @param claims The row entity's statements as they stand, by property
 * @param reference The reference a new statement carries, or null
 * @return The data, null when nothing changed, or the cell that is not a value of its type
 */
export function rowEdit(
	columns: TableColumn[],
	before: TableRow | null,
	after: string[],
	claims: Record<string, Claim[]>,
	reference: Record<string, unknown> | null,
): Record<string, unknown> | CellProblem | null {
	const data: Record<string, unknown> = {};
	const labels: Record<string, unknown> = {};
	const descriptions: Record<string, unknown> = {};
	const aliases: unknown[] = [];
	const edits: unknown[] = [];
	for ( let c = 0; c < columns.length; c++ ) {
		const col = columns[ c ];
		const was = before ? before.cells[ c ] : undefined;
		const text = ( after[ c ] ?? '' ).trim();
		if ( ( was && ( !was.editable || was.text === text ) ) || ( !was && text === '' ) ) {
			continue;
		}
		const language = col.language ?? '';
		if ( col.kind === 'label' || col.kind === 'description' ) {
			const term = text === '' ? { language, remove: '' } : { language, value: text };
			( col.kind === 'label' ? labels : descriptions )[ language ] = term;
		} else if ( col.kind === 'aliases' ) {
			const list = splitValues( text );
			// A list with no flags replaces the language's aliases; one empty value clears them.
			aliases.push( ...( list.length ? list : [ '' ] ).map( ( value ) => ( { language, value } ) ) );
		} else if ( col.kind === 'statements' && col.property ) {
			if ( text === '' ) {
				if ( was?.guid ) {
					edits.push( { id: was.guid, remove: '' } );
				}
				continue;
			}
			const datavalue = dataValueOf( col.datatype ?? '', text );
			if ( !datavalue ) {
				return { column: c, text };
			}
			const mainsnak = { snaktype: 'value', property: col.property, datavalue };
			const existing = was?.guid ?
				( claims[ col.property ] ?? [] ).find( ( s ) => s.id === was.guid ) :
				undefined;
			if ( existing ) {
				edits.push( { ...existing, mainsnak } );
			} else {
				const claim: Record<string, unknown> = { mainsnak, type: 'statement', rank: 'normal' };
				if ( reference ) {
					claim.references = [ reference ];
				}
				edits.push( claim );
			}
		}
	}
	if ( Object.keys( labels ).length ) {
		data.labels = labels;
	}
	if ( Object.keys( descriptions ).length ) {
		data.descriptions = descriptions;
	}
	if ( aliases.length ) {
		data.aliases = aliases;
	}
	if ( edits.length ) {
		data.claims = edits;
	}
	return Object.keys( data ).length ? data : null;
}

/**
 * Whether a row edit is a problem rather than data.
 *
 * @param e What `rowEdit` gave
 * @return Whether it names a cell
 */
export function isProblem( e: Record<string, unknown> | CellProblem | null ): e is CellProblem {
	return e !== null && typeof ( e as CellProblem ).column === 'number' && typeof ( e as CellProblem ).text === 'string';
}

/**
 * The reference a new statement carries: the definition's default reference, where every
 * snak holds a value. One that reads a column is a draft's, for staged rows.
 *
 * @param definition The definition, parsed
 * @return The reference, or null
 */
export function defaultReference(
	definition: Record<string, unknown>,
): Record<string, unknown> | null {
	type Snaks = Record<string, Record<string, unknown>[]>;
	const ref = definition.default_reference as { snaks?: Snaks } | undefined;
	if ( !ref || !ref.snaks ) {
		return null;
	}
	const whole = Object.values( ref.snaks )
		.every( ( list ) => list.every( ( s ) => s.column === undefined ) );
	return whole ? ref as Record<string, unknown> : null;
}

/**
 * The definition with rows added and removed, as text for `action=edit`; the server
 * writes it canonically.
 *
 * @param definition The definition as stored
 * @param add The IDs to add, in order
 * @param remove The IDs to remove
 * @return The new text, or null when the rows are the same
 */
export function definitionEdit(
	definition: string,
	add: string[],
	remove: string[],
): string | null {
	const def = JSON.parse( definition ) as { rows: { ids: string[] } };
	const gone = new Set( remove.map( ( id ) => id.toUpperCase() ) );
	const ids = def.rows.ids.filter( ( id ) => !gone.has( id.toUpperCase() ) );
	for ( const id of add ) {
		if ( !ids.some( ( x ) => x.toUpperCase() === id.toUpperCase() ) ) {
			ids.push( id );
		}
	}
	const same = ids.length === def.rows.ids.length &&
		ids.every( ( id, i ) => id === def.rows.ids[ i ] );
	if ( same ) {
		return null;
	}
	def.rows.ids = ids;
	return JSON.stringify( def, null, 2 );
}
