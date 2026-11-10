import { describe, expect, it } from 'vitest';
import { column, headerOf, joinValues, rowRef, rowsOf, splitValues, textOf } from './text';
import { dataValueOf, plainOf } from './plain';

// The vectors of create_entities/table.rs, so the browser and the server read alike.
describe( 'the table\'s text', () => {
	it( 'splits as tabs or commas', () => {
		expect( rowsOf( 'label\tP31\nSix\tQ5\n\nSeven\tQ6\n' ) )
			.toEqual( [ [ 'label', 'P31' ], [ 'Six', 'Q5' ], [ 'Seven', 'Q6' ] ] );
		expect( rowsOf( '﻿label,description:de\r\n"Six, the number","eine ""Zahl"""\r\nSeven,\r\n' ) )
			.toEqual( [ [ 'label', 'description:de' ], [ 'Six, the number', 'eine "Zahl"' ], [ 'Seven', '' ] ] );
	} );

	it( 'round-trips', () => {
		const rows = [
			[ 'id', 'label', 'new:Publisher@wikibase-item', 'P577/ref:P854' ],
			[ '', 'Six, the "number"', 'row:2', 'https://example.org/a|b' ],
			[ 'Q5', '!Seven', '', '' ],
			[ 'skip', 'two\nlines', 'a\\|b|c', '' ],
		];
		expect( rowsOf( textOf( rows, false ) ) ).toEqual( rows );
		expect( rowsOf( textOf( rows, true ) ) ).toEqual( rows );
		const simple = rows.slice( 0, 3 );
		expect( textOf( simple, true ) ).toContain( '\t' );
		expect( rowsOf( textOf( simple, true ) ) ).toEqual( simple );
		const one = [ [ 'label' ], [ 'a, b' ] ];
		expect( rowsOf( textOf( one, true ) ) ).toEqual( one );
	} );

	it( 'reads and writes headers', () => {
		expect( column( 'label', 'en' ) ).toEqual( { kind: 'label', language: 'en' } );
		expect( column( 'Description:de', 'en' ) ).toEqual( { kind: 'description', language: 'de' } );
		expect( column( 'p31', 'en' ) ).toEqual( { kind: 'property', property: 'P31' } );
		expect( column( 'ID', 'en' ) ).toEqual( { kind: 'id' } );
		expect( column( 'New:Date of first issue@time', 'en' ) )
			.toEqual( { kind: 'new', label: 'Date of first issue', datatype: 'time' } );
		expect( column( 'p577/ref:p854', 'en' ) ).toEqual( { kind: 'reference', of: 'P577', property: 'P854' } );
		expect( column( 'label:not a language', 'en' ).kind ).toBe( 'plain' );
		expect( column( 'source url', 'en' ) ).toEqual( { kind: 'plain', label: 'source url' } );
		for ( const h of [ 'id', 'label', 'description:de', 'aliases:fr', 'P31', 'new:Publisher',
			'new:Date of first issue@time', 'P577/ref:P854', 'source url' ] ) {
			expect( headerOf( column( h, 'en' ), 'en' ) ).toBe( h );
		}
	} );

	it( 'holds several values in a cell', () => {
		expect( splitValues( 'a| b |' ) ).toEqual( [ 'a', 'b' ] );
		expect( splitValues( 'a\\|b|c' ) ).toEqual( [ 'a|b', 'c' ] );
		expect( splitValues( joinValues( [ 'a|b', 'c' ] ) ) ).toEqual( [ 'a|b', 'c' ] );
		expect( rowRef( 'row:14' ) ).toBe( 14 );
		expect( rowRef( 'row:0' ) ).toBeNull();
		expect( rowRef( 'Q5' ) ).toBeNull();
	} );
} );

// The plain forms of special/values.rs.
describe( 'plain values', () => {
	it( 'read back as they were written', () => {
		for ( const [ datatype, text ] of [
			[ 'string', 'a|b' ], [ 'url', 'https://example.org/a' ], [ 'wikibase-item', 'Q5' ],
			[ 'quantity', '12' ], [ 'quantity', '-3.5' ], [ 'time', '1969' ], [ 'time', '1969-07' ],
			[ 'time', '1969-07-20' ], [ 'monolingualtext', 'Hallo@de' ], [ 'globe-coordinate', '45.5, -122.6' ],
		] ) {
			const dv = dataValueOf( datatype, text );
			expect( dv, `${ datatype } ${ text }` ).not.toBeNull();
			expect( plainOf( dv ?? undefined ) ).toBe( text );
		}
	} );

	it( 'refuse what is not a value of the type', () => {
		expect( dataValueOf( 'wikibase-item', 'journal' ) ).toBeNull();
		expect( dataValueOf( 'wikibase-item', 'P5' ) ).toBeNull();
		expect( dataValueOf( 'quantity', 'twelve' ) ).toBeNull();
		expect( dataValueOf( 'time', 'March 1952' ) ).toBeNull();
		expect( dataValueOf( 'globe-coordinate', '95, 0' ) ).toBeNull();
		expect( dataValueOf( 'string', '  ' ) ).toBeNull();
		expect( plainOf( { type: 'quantity', value: { amount: '+1', unit: '1', upperBound: '+2' } } ) ).toBeNull();
	} );

	// 0106 A7: a quantity's unit follows it, as its item ID under the site's base.
	it( 'write a quantity\'s unit after it', () => {
		const base = 'https://wiki.example/entity/';
		const dv = dataValueOf( 'quantity', '12 q11573', base );
		expect( dv ).toEqual( { type: 'quantity', value: { amount: '+12', unit: `${ base }Q11573` } } );
		expect( plainOf( dv ?? undefined ) ).toBe( '12 Q11573' );
		expect( dataValueOf( 'quantity', '12 metres', base ) ).toBeNull();
		expect( dataValueOf( 'quantity', '12 Q5' ) ).toBeNull();
	} );
} );
