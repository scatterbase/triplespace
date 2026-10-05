/**
 * A statement group as the group editor holds it, and the `wbeditentity` claims that
 * save it in one revision (ADR 0003 §8). Every statement of the group is sent, in order,
 * changed or not, so the group keeps its order; removed ones are sent with `remove`.
 */

import { emptyForm, formOf, isInvalid, snakOf } from './values';
import type { Form, Snak } from './values';

/** A reference's Wikibase JSON. */
export interface Reference {
	hash?: string;
	snaks: Record<string, Snak[]>;
	'snaks-order'?: string[];
}

/** A statement's Wikibase JSON. */
export interface Statement {
	id?: string;
	type?: string;
	mainsnak: Snak;
	rank: 'preferred' | 'normal' | 'deprecated';
	qualifiers?: Record<string, Snak[]>;
	'qualifiers-order'?: string[];
	references?: Reference[];
}

/** One snak in the editor: a property, its data type, the form, and where it came from. */
export interface SnakRow {
	property: string;
	datatype: string;
	form: Form;
	original?: Snak;
}

/** One reference in the editor. */
export interface ReferenceRow {
	snaks: SnakRow[];
	original?: Reference;
}

/** One statement in the editor. */
export interface StatementRow {
	key: string;
	id?: string;
	main: SnakRow;
	rank: Statement[ 'rank' ];
	qualifiers: SnakRow[];
	references: ReferenceRow[];
	removed: boolean;
	original?: Statement;
}

/** What the editor needs to know of the site. */
export interface Context {
	conceptBase: string;
	datatypes: Record<string, string>;
}

let counter = 0;

/**
 * A fresh key for a row the editor adds.
 *
 * @return The key
 */
export function freshKey(): string {
	counter++;
	return `new-${ counter }`;
}

function snakRow( snak: Snak, cx: Context ): SnakRow {
	return {
		property: snak.property,
		datatype: snak.datatype ?? cx.datatypes[ snak.property ] ?? 'string',
		form: formOf( snak, cx.conceptBase ),
		original: snak,
	};
}

function grouped( snaks: Record<string, Snak[]> | undefined, order: string[] | undefined ): Snak[] {
	if ( !snaks ) {
		return [];
	}
	const rest = Object.keys( snaks ).filter( ( k ) => !order?.includes( k ) );
	const keys = [ ...( order ?? [] ), ...rest ];
	return keys.flatMap( ( k ) => snaks[ k ] ?? [] );
}

/**
 * A statement's editor row.
 *
 * @param s The statement
 * @param cx The context
 * @return The row
 */
export function rowOf( s: Statement, cx: Context ): StatementRow {
	return {
		key: s.id ?? freshKey(),
		id: s.id,
		main: snakRow( s.mainsnak, cx ),
		rank: s.rank,
		qualifiers: grouped( s.qualifiers, s[ 'qualifiers-order' ] ).map( ( q ) => snakRow( q, cx ) ),
		references: ( s.references ?? [] ).map( ( r ) => ( {
			snaks: grouped( r.snaks, r[ 'snaks-order' ] ).map( ( q ) => snakRow( q, cx ) ),
			original: r,
		} ) ),
		removed: false,
		original: s,
	};
}

/**
 * A new, empty row for a property.
 *
 * @param property The property
 * @param datatype Its data type
 * @return The row
 */
export function newRow( property: string, datatype: string ): StatementRow {
	return {
		key: freshKey(),
		main: { property, datatype, form: emptyForm() },
		rank: 'normal',
		qualifiers: [],
		references: [],
		removed: false,
	};
}

/**
 * A new snak row for a property.
 *
 * @param property The property
 * @param datatype Its data type
 * @return The row
 */
export function newSnak( property: string, datatype: string ): SnakRow {
	return { property, datatype, form: emptyForm() };
}

/** A row the editor cannot save, and why. */
export interface RowProblem {
	key: string;
	problem: string;
}

function snakOfRow( r: SnakRow, cx: Context ): Snak | string {
	const s = snakOf( r.form, r.property, r.datatype, cx.conceptBase, r.original );
	return isInvalid( s ) ? s.problem : s;
}

function group( snaks: Snak[] ): [ Record<string, Snak[]>, string[] ] {
	const out: Record<string, Snak[]> = {};
	const order: string[] = [];
	for ( const s of snaks ) {
		if ( !out[ s.property ] ) {
			out[ s.property ] = [];
			order.push( s.property );
		}
		out[ s.property ].push( s );
	}
	return [ out, order ];
}

function unchangedSnaks( rows: SnakRow[], built: Snak[] ): boolean {
	return rows.every( ( r, i ) => r.original !== undefined && built[ i ] === r.original );
}

/**
 * The statement a row describes: the original where nothing changed.
 *
 * @param row The row
 * @param cx The context
 * @return The statement, or the first problem
 */
export function statementOf( row: StatementRow, cx: Context ): Statement | RowProblem {
	const main = snakOfRow( row.main, cx );
	if ( typeof main === 'string' ) {
		return { key: row.key, problem: main };
	}
	const qualifiers: Snak[] = [];
	for ( const qr of row.qualifiers ) {
		const built = snakOfRow( qr, cx );
		if ( typeof built === 'string' ) {
			return { key: row.key, problem: built };
		}
		qualifiers.push( built );
	}
	const references: Reference[] = [];
	for ( const r of row.references ) {
		const snaks: Snak[] = [];
		for ( const sr of r.snaks ) {
			const built = snakOfRow( sr, cx );
			if ( typeof built === 'string' ) {
				return { key: row.key, problem: built };
			}
			snaks.push( built );
		}
		if ( snaks.length === 0 ) {
			continue;
		}
		const sameAsBefore = r.original &&
			grouped( r.original.snaks, r.original[ 'snaks-order' ] ).length === snaks.length &&
			unchangedSnaks( r.snaks, snaks );
		if ( sameAsBefore && r.original ) {
			references.push( r.original );
		} else {
			const [ g, snakOrder ] = group( snaks );
			references.push( { snaks: g, 'snaks-order': snakOrder } );
		}
	}
	const o = row.original;
	const unchanged = o !== undefined && main === o.mainsnak && row.rank === o.rank &&
		qualifiers.length === grouped( o.qualifiers, o[ 'qualifiers-order' ] ).length &&
		unchangedSnaks( row.qualifiers, qualifiers ) &&
		references.length === ( o.references ?? [] ).length &&
		references.every( ( r, i ) => r === ( o.references ?? [] )[ i ] );
	if ( unchanged && o ) {
		return o;
	}
	const [ q, order ] = group( qualifiers );
	const s: Statement = { type: 'statement', mainsnak: main, rank: row.rank };
	if ( row.id ) {
		s.id = row.id;
	}
	if ( qualifiers.length > 0 ) {
		s.qualifiers = q;
		s[ 'qualifiers-order' ] = order;
	}
	if ( references.length > 0 ) {
		s.references = references;
	}
	return s;
}

/**
 * The `claims` of the `wbeditentity` that saves a group, in order: every statement,
 * and `remove` for the removed ones. A new row left empty is dropped.
 *
 * @param rows The rows
 * @param cx The context
 * @return The claims and whether anything changed, or the first problem
 */
export function claimsEdit(
	rows: StatementRow[],
	cx: Context,
): { claims: unknown[]; changed: boolean } | RowProblem {
	const claims: unknown[] = [];
	let changed = false;
	for ( const row of rows ) {
		if ( row.removed ) {
			if ( row.id ) {
				claims.push( { id: row.id, remove: '' } );
				changed = true;
			}
			continue;
		}
		const pristine = !row.original && row.main.form.snaktype === 'value' &&
			JSON.stringify( row.main.form ) === JSON.stringify( emptyForm() ) &&
			row.qualifiers.length === 0 && row.references.length === 0;
		if ( pristine ) {
			continue;
		}
		const s = statementOf( row, cx );
		if ( 'problem' in s ) {
			return s;
		}
		if ( s !== row.original ) {
			changed = true;
		}
		claims.push( s );
	}
	return { claims, changed };
}
