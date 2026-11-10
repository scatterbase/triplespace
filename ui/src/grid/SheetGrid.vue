<template>
	<div class="ts-sheet">
		<table class="cdx-table__table ts-sheet__table">
			<thead>
				<tr>
					<th scope="col" class="ts-sheet__lead-heading">
						{{ leadHeading }}
					</th>
					<th
						v-for="( h, c ) in headers"
						:key="c"
						scope="col"
						:class="{ 'ts-sheet__heading--active': active && active.c === c }">
						<span class="ts-sheet__heading">{{ h.label }}</span>
						<span v-if="h.sub" class="ts-sheet__sub">{{ h.sub }}</span>
						<button
							v-if="h.menu"
							type="button"
							class="ts-sheet__menu"
							:aria-label="menuLabel( h.label )"
							:title="menuLabel( h.label )"
							@click="emit( 'menu', c )">
							⋯
						</button>
					</th>
				</tr>
			</thead>
			<tbody>
				<tr
					v-for="( row, r ) in rows"
					:key="row.key"
					:class="row.kind ? `ts-sheet__row--${ row.kind }` : undefined">
					<th scope="row" class="ts-sheet__lead">
						<a v-if="row.href" :href="row.href">{{ row.lead }}</a>
						<span v-else>{{ row.lead }}</span>
						<span
							v-if="row.status"
							class="ts-sheet__status"
							:class="statusClass( row )">{{ row.status }}</span>
						<button
							v-if="row.removable"
							type="button"
							class="ts-sheet__remove"
							:aria-label="removeLabel( row.lead )"
							:title="removeLabel( row.lead )"
							@click="emit( 'remove', row.key )">
							×
						</button>
						<ul v-if="row.problems && row.problems.length" class="ts-sheet__problems">
							<li v-for="( p, i ) in row.problems" :key="i">
								{{ p }}
							</li>
						</ul>
					</th>
					<td
						v-for="( cell, c ) in row.cells"
						:key="c"
						:class="{
							'ts-sheet__cell--changed': row.changed && row.changed[ c ],
							'ts-sheet__cell--readonly': row.readonly && row.readonly[ c ]
						}">
						<template v-if="row.readonly && row.readonly[ c ]">
							<span>{{ cell }}</span>
						</template>
						<input
							v-else
							class="ts-sheet__input"
							type="text"
							:value="cell"
							:data-r="r"
							:data-c="c"
							:aria-label="cellLabel( headingOf( c ), row.lead )"
							@input="typed( $event, row.key, c )"
							@focus="focus( row.key, r, c )"
							@keydown="key( $event, row.key, r, c )"
							@paste="paste( $event, row.key, c )">
						<span
							v-if="row.notes && row.notes[ c ]"
							class="ts-sheet__note">{{ row.notes[ c ] }}</span>
					</td>
				</tr>
			</tbody>
		</table>
	</div>
</template>

<script setup lang="ts">
// A grid of text cells with a leading cell per row, shared by the table of
// Special:CreateManyEntities and a Table page's editor (ADR 0106 §6; 0045 §6). It holds
// no state of its own: each keystroke, paste and fill is reported, and the rows it is
// given are what it shows. Enter and the up and down arrows move between rows; Ctrl+D
// fills a cell from the one above; pasting a range from a spreadsheet is reported whole.
import { ref } from 'vue';

import type { SheetHeader, SheetRow, Words, Words2 } from './sheet';

const props = defineProps<{
	headers: SheetHeader[];
	rows: SheetRow[];
	leadHeading: string;
	cellLabel: Words2;
	menuLabel: Words;
	removeLabel: Words;
}>();

const emit = defineEmits<{
	edit: [ key: string, column: number, value: string ];
	paste: [ key: string, column: number, cells: string[][] ];
	fill: [ key: string, column: number ];
	menu: [ column: number ];
	remove: [ key: string ];
	focus: [ key: string, column: number ];
}>();

const active = ref<{ r: number; c: number } | null>( null );

const headingOf = ( c: number ): string => props.headers[ c ]?.label ?? '';
const statusClass = ( row: SheetRow ): string | undefined => ( row.kind ?
	`ts-sheet__status--${ row.kind }` :
	undefined );

function typed( e: Event, rowKey: string, c: number ): void {
	emit( 'edit', rowKey, c, ( e.target as HTMLInputElement ).value );
}

function focus( rowKey: string, r: number, c: number ): void {
	active.value = { r, c };
	emit( 'focus', rowKey, c );
}

function move( from: HTMLElement, r: number, c: number ): void {
	const table = from.closest( 'table' );
	const next = table?.querySelector<HTMLInputElement>( `input[data-r="${ r }"][data-c="${ c }"]` );
	if ( next ) {
		next.focus();
		next.select();
	}
}

function key( e: KeyboardEvent, rowKey: string, r: number, c: number ): void {
	const el = e.target as HTMLElement;
	if ( e.key === 'Enter' || e.key === 'ArrowDown' ) {
		e.preventDefault();
		move( el, r + 1, c );
	} else if ( e.key === 'ArrowUp' ) {
		e.preventDefault();
		move( el, r - 1, c );
	} else if ( ( e.ctrlKey || e.metaKey ) && e.key.toLowerCase() === 'd' ) {
		e.preventDefault();
		emit( 'fill', rowKey, c );
	}
}

function paste( e: ClipboardEvent, rowKey: string, c: number ): void {
	const text = e.clipboardData?.getData( 'text/plain' ) ?? '';
	if ( !/[\t\n]/.test( text.replace( /\r?\n$/, '' ) ) ) {
		return;
	}
	e.preventDefault();
	const cells = text.replace( /\r/g, '' ).replace( /\n$/, '' ).split( '\n' ).map( ( l ) => l.split( '\t' ) );
	emit( 'paste', rowKey, c, cells );
}
</script>
