import { describe, expect, it } from 'vitest';
import { claimsEdit, newRow, newSnak, rowOf } from './statements';
import type { Statement } from './statements';
import { EARTH, GREGORIAN, emptyForm, formOf, isInvalid, kindOf, parseTime, snakOf, timeText } from './values';
import type { Snak } from './values';

const base = 'https://librarybase.org/entity/';
const cx = { conceptBase: base, datatypes: { P9: 'time', P10: 'wikibase-item', P11: 'url' } };

describe( 'times', () => {
	it( 'read and write by precision', () => {
		expect( timeText( '+1952-03-11T00:00:00Z', 11 ) ).toBe( '1952-03-11' );
		expect( timeText( '+1952-03-00T00:00:00Z', 10 ) ).toBe( '1952-03' );
		expect( timeText( '+2020-00-00T00:00:00Z', 9 ) ).toBe( '2020' );
		expect( timeText( '-0044-03-15T00:00:00Z', 9 ) ).toBe( '-0044' );
		expect( parseTime( '1952-03-11' ) ).toEqual( { time: '+1952-03-11T00:00:00Z', precision: 11 } );
		expect( parseTime( '2020' ) ).toEqual( { time: '+2020-00-00T00:00:00Z', precision: 9 } );
		expect( parseTime( '-44' ) ).toEqual( { time: '-0044-00-00T00:00:00Z', precision: 9 } );
		expect( parseTime( '1952-13' ) ).toBeNull();
		expect( parseTime( 'March 1952' ) ).toBeNull();
	} );
} );

describe( 'snaks', () => {
	it( 'give back the original when the form is unchanged', () => {
		const s: Snak = { snaktype: 'value', property: 'P3', datatype: 'quantity', hash: 'h',
			datavalue: { type: 'quantity', value: { amount: '+10', unit: `${ base }Q5`, upperBound: '+11', lowerBound: '+9' } } };
		const f = formOf( s, base );
		expect( f.amount ).toBe( '10' );
		expect( f.unit ).toBe( 'Q5' );
		expect( snakOf( f, 'P3', 'quantity', base, s ) ).toBe( s );
		const changed = snakOf( { ...f, amount: '12' }, 'P3', 'quantity', base, s );
		expect( changed ).toEqual( { snaktype: 'value', property: 'P3', datatype: 'quantity',
			datavalue: { type: 'quantity', value: { amount: '+12', unit: `${ base }Q5` } } } );
	} );

	it( 'build each kind of value', () => {
		const f = emptyForm();
		expect( kindOf( 'external-id' ) ).toBe( 'text' );
		expect( kindOf( 'wikibase-lexeme' ) ).toBe( 'unsupported' );
		expect( snakOf( { ...f, id: 'Q1' }, 'P2', 'wikibase-item', base ) ).toMatchObject(
			{ datavalue: { type: 'wikibase-entityid', value: { 'entity-type': 'item', id: 'Q1' } } } );
		expect( snakOf( { ...f, text: '1952' }, 'P4', 'time', base ) ).toMatchObject(
			{ datavalue: { value: { time: '+1952-00-00T00:00:00Z', precision: 9, calendarmodel: GREGORIAN } } } );
		expect( snakOf( { ...f, lat: '34.05', lon: '-118.25' }, 'P5', 'globe-coordinate', base ) ).toMatchObject(
			{ datavalue: { value: {
				latitude: 34.05, longitude: -118.25, precision: 0.01, globe: EARTH,
			} } } );
		expect( snakOf( { ...f, text: 'ستة', lang: 'AR' }, 'P8', 'monolingualtext', base ) ).toMatchObject(
			{ datavalue: { value: { text: 'ستة', language: 'ar' } } } );
		expect( snakOf( { ...f, snaktype: 'somevalue' }, 'P10', 'wikibase-item', base ) ).toEqual(
			{ snaktype: 'somevalue', property: 'P10', datatype: 'wikibase-item' } );
		const bad = [
			[ f, 'string' ],
			[ { ...f, id: 'nope' }, 'wikibase-item' ],
			[ { ...f, amount: 'ten' }, 'quantity' ],
			[ { ...f, lat: '95', lon: '0' }, 'globe-coordinate' ],
		] as const;
		for ( const [ form, dt ] of bad ) {
			expect( isInvalid( snakOf( form, 'P1', dt, base ) ) ).toBe( true );
		}
	} );
} );

describe( 'groups', () => {
	const one: Statement = { id: 'Q6$1', type: 'statement', rank: 'normal',
		mainsnak: { snaktype: 'value', property: 'P2', datatype: 'wikibase-item', datavalue: { type: 'wikibase-entityid', value: { 'entity-type': 'item', id: 'Q1' } } } };
	const two: Statement = { id: 'Q6$2', type: 'statement', rank: 'normal',
		mainsnak: { snaktype: 'value', property: 'P2', datatype: 'wikibase-item', datavalue: { type: 'wikibase-entityid', value: { 'entity-type': 'item', id: 'Q2' } } },
		qualifiers: { P9: [ { snaktype: 'value', property: 'P9', datavalue: { type: 'time', value: { time: '+2020-00-00T00:00:00Z', precision: 9 } } } ] },
		'qualifiers-order': [ 'P9' ],
		references: [ { hash: 'r', snaks: { P11: [ { snaktype: 'value', property: 'P11', datavalue: { type: 'string', value: 'https://example.org/' } } ] }, 'snaks-order': [ 'P11' ] } ] };

	it( 'send every statement in order, unchanged ones as they were', () => {
		const rows = [ rowOf( one, cx ), rowOf( two, cx ) ];
		const r = claimsEdit( rows, cx );
		expect( r ).toEqual( { claims: [ one, two ], changed: false } );
	} );

	it( 'send changes, additions and removals in one edit', () => {
		const rows = [ rowOf( one, cx ), rowOf( two, cx ) ];
		rows[ 0 ].removed = true;
		rows[ 1 ].rank = 'preferred';
		rows[ 1 ].references[ 0 ].snaks.push( newSnak( 'P11', 'url' ) );
		rows[ 1 ].references[ 0 ].snaks[ 1 ].form.text = 'https://example.com/';
		const added = newRow( 'P2', 'wikibase-item' );
		added.main.form.id = 'Q3';
		added.qualifiers.push( newSnak( 'P9', 'time' ) );
		added.qualifiers[ 0 ].form.text = '2021';
		rows.push( added, newRow( 'P2', 'wikibase-item' ) );
		const r = claimsEdit( rows, cx );
		expect( 'claims' in r && r.changed ).toBe( true );
		if ( !( 'claims' in r ) ) {
			return;
		}
		expect( r.claims ).toHaveLength( 3 );
		expect( r.claims[ 0 ] ).toEqual( { id: 'Q6$1', remove: '' } );
		const changed = r.claims[ 1 ] as Statement;
		expect( changed.rank ).toBe( 'preferred' );
		expect( changed.id ).toBe( 'Q6$2' );
		expect( changed.qualifiers?.P9[ 0 ] ).toBe( two.qualifiers?.P9[ 0 ] );
		expect( changed.references?.[ 0 ].hash ).toBeUndefined();
		expect( changed.references?.[ 0 ].snaks.P11 ).toHaveLength( 2 );
		const fresh = r.claims[ 2 ] as Statement;
		expect( fresh.id ).toBeUndefined();
		expect( fresh[ 'qualifiers-order' ] ).toEqual( [ 'P9' ] );
	} );

	it( 'report the first value it cannot save', () => {
		const bad = newRow( 'P11', 'url' );
		bad.main.form.lang = 'x';
		bad.qualifiers.push( newSnak( 'P9', 'time' ) );
		bad.qualifiers[ 0 ].form.text = 'soon';
		expect( claimsEdit( [ bad ], cx ) ).toEqual( { key: bad.key, problem: 'ts-edit-need-value' } );
	} );
} );
