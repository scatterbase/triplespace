/* eslint-disable camelcase -- the definition's keys are the content model's */
import { describe, expect, it } from 'vitest';
import { apply, suggest } from './mapping';
import { defaultReference, definitionEdit, isProblem, rowEdit } from './tableedit';
import type { TableColumn, TableRow } from './data';

const columns: TableColumn[] = [
	{ kind: 'label', label: 'Label', sub: 'en', language: 'en' },
	{ kind: 'aliases', label: 'Aliases', sub: 'en', language: 'en' },
	{ kind: 'statements', label: 'instance of', sub: 'P31', property: 'P31', datatype: 'wikibase-item' },
	{ kind: 'statements', label: 'published', sub: 'P577', property: 'P577', datatype: 'time' },
	{ kind: 'other', label: 'en.wikipedia.org', sub: '' },
];
type Cell = TableRow[ 'cells' ][ number ];
const cell = ( text: string, guid?: string, editable = true ): Cell => ( { text, note: '', editable, guid } );
const row: TableRow = {
	id: 'Q1', href: '/wiki/Item:Q1', lastrevid: 7, missing: false,
	cells: [ cell( 'One' ), cell( 'uno' ), cell( 'Q5', 'Q1$a' ), cell( '' ), cell( 'One', undefined, false ) ],
};

// 0045 §6: what a cell edit becomes.
describe( 'a row\'s edit', () => {
	it( 'is nothing when nothing changed', () => {
		expect( rowEdit( columns, row, [ 'One', 'uno', 'Q5', '', 'x' ], {}, null ) ).toBeNull();
	} );

	it( 'sets and removes terms, and replaces the language\'s aliases', () => {
		expect( rowEdit( columns, row, [ 'Uno', '', 'Q5', '', '' ], {}, null ) ).toEqual( {
			labels: { en: { language: 'en', value: 'Uno' } },
			aliases: [ { language: 'en', value: '' } ],
		} );
		expect( rowEdit( columns, row, [ '', 'a|b', 'Q5', '', '' ], {}, null ) ).toEqual( {
			labels: { en: { language: 'en', remove: '' } },
			aliases: [ { language: 'en', value: 'a' }, { language: 'en', value: 'b' } ],
		} );
	} );

	it( 'changes the one value keeping the statement, adds with the reference, and removes', () => {
		const claims = { P31: [ { id: 'Q1$a', mainsnak: { snaktype: 'value', property: 'P31' }, rank: 'preferred', qualifiers: { P1: [] } } ] };
		const reference = { snaks: { P248: [ { snaktype: 'value', property: 'P248' } ] } };
		const data = rowEdit( columns, row, [ 'One', 'uno', 'q6', '1969-07', '' ], claims, reference ) as { claims: Record<string, unknown>[] };
		expect( data.claims ).toHaveLength( 2 );
		expect( data.claims[ 0 ] ).toMatchObject( { id: 'Q1$a', rank: 'preferred', qualifiers: { P1: [] } } );
		expect( data.claims[ 0 ].mainsnak ).toMatchObject( { datavalue: { value: { id: 'Q6' } } } );
		expect( data.claims[ 1 ] ).toMatchObject( { rank: 'normal', references: [ reference ] } );
		expect( data.claims[ 1 ].id ).toBeUndefined();
		expect( rowEdit( columns, row, [ 'One', 'uno', '', '', '' ], claims, null ) ).toEqual( { claims: [ { id: 'Q1$a', remove: '' } ] } );
	} );

	it( 'names a cell that is not a value of its type', () => {
		const bad = rowEdit( columns, row, [ 'One', 'uno', 'journal', '', '' ], {}, null );
		expect( isProblem( bad ) && bad.column ).toBe( 2 );
	} );

	it( 'is a new item from a new row\'s cells', () => {
		expect( rowEdit( columns, null, [ 'New', '', 'Q5', '', 'ignored' ], {}, null ) ).toEqual( {
			labels: { en: { language: 'en', value: 'New' } },
			claims: [ { mainsnak: { snaktype: 'value', property: 'P31', datavalue: { type: 'wikibase-entityid', value: { 'entity-type': 'item', id: 'Q5' } } }, type: 'statement', rank: 'normal' } ],
		} );
		expect( rowEdit( columns, null, [ '', '', '', '', '' ], {}, null ) ).toBeNull();
	} );
} );

// 0045 §7: rows are part of the definition.
describe( 'the definition', () => {
	const stored = JSON.stringify( { version: 1, rows: { ids: [ 'Q1', 'Q2' ] }, columns: [], default_reference: { snaks: { P248: [ { snaktype: 'value', datavalue: {} } ] } } } );

	it( 'gains and loses rows', () => {
		expect( definitionEdit( stored, [], [] ) ).toBeNull();
		expect( definitionEdit( stored, [ 'Q1' ], [] ) ).toBeNull();
		const text = definitionEdit( stored, [ 'Q9', 'q2' ], [ 'q1' ] );
		expect( JSON.parse( text ?? '{}' ).rows.ids ).toEqual( [ 'Q2', 'Q9' ] );
	} );

	it( 'gives its default reference to new statements, unless it reads a column', () => {
		expect( defaultReference( JSON.parse( stored ) ) ).not.toBeNull();
		expect( defaultReference( { default_reference: { snaks: { P854: [ { snaktype: 'value', column: 3 } ] } } } ) ).toBeNull();
		expect( defaultReference( {} ) ).toBeNull();
	} );
} );

// 0106 §7: what an uploaded file's columns become.
describe( 'a mapping', () => {
	it( 'suggests what is evident', () => {
		expect( suggest( 'Title', 'en' ).target ).toBe( 'label' );
		expect( suggest( 'description:de', 'en' ) ).toMatchObject( { target: 'description', language: 'de' } );
		expect( suggest( 'p31', 'en' ) ).toMatchObject( { target: 'property', property: 'P31' } );
		expect( suggest( 'P577/ref:P854', 'en' ) ).toMatchObject( { target: 'reference', of: 'P577', property: 'P854' } );
		expect( suggest( 'publisher', 'en' ).target ).toBe( 'ignore' );
	} );

	it( 'makes the table and the reference lines', () => {
		const m = ( over: object ): ReturnType<typeof suggest> => ( { ...suggest( '', 'en' ), ...over } );
		const out = apply(
			[ 'Name', 'Kind', 'junk', 'URL', 'label' ],
			[ [ 'A', 'Q5', 'x', 'https://a', 'z' ], [ 'B', 'Q6', 'y' ] ],
			[ m( { target: 'label', language: 'de' } ), m( { target: 'property', property: 'p31' } ), m( { target: 'ignore' } ),
				m( { target: 'reads', property: 'P854' } ), m( { target: 'reads', property: 'P1476' } ) ],
			'en',
		);
		expect( out.rows ).toEqual( [
			[ 'label:de', 'P31', 'URL', 'label (source)' ],
			[ 'A', 'Q5', 'https://a', 'z' ],
			[ 'B', 'Q6', '', '' ],
		] );
		expect( out.reference ).toEqual( [ 'P854:{URL}', 'P1476:{label (source)}' ] );
	} );
} );
